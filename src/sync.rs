//! Git sync: commit the vault, never push it.
//!
//! DayDoc stages and commits; pushing is the user's git credential helper's job,
//! so DayDoc never holds a token. There is no merge: two writers produce a
//! conflict, and a conflict is a `.conflict.md` sidecar the user resolves.

use git2::{IndexAddOption, Repository, Signature};

use crate::error::{DayDocError, Result};
use crate::vault::Vault;

pub const DEFAULT_MESSAGE: &str = "daydoc: autosave";

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub changed: Vec<String>,
    pub committed: bool,
    pub message: String,
    pub head: Option<String>,
}

impl SyncReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        if !self.committed {
            out.push_str("nothing to commit\n");
            return out;
        }
        out.push_str(&format!("committed {} file(s): {}\n", self.changed.len(), self.message));
        if let Some(head) = &self.head {
            out.push_str(&format!("  HEAD {head}\n"));
        }
        for path in self.changed.iter().take(20) {
            out.push_str(&format!("  {path}\n"));
        }
        out.push_str("push with your own git credential helper: git push\n");
        out
    }
}

fn git_err(err: git2::Error) -> DayDocError {
    DayDocError::Git(err.message().to_string())
}

fn open_repo(vault: &Vault) -> Result<Repository> {
    Repository::open(vault.root()).map_err(|err| {
        DayDocError::Git(format!(
            "no git repository at {}: {}; run `git init` there first",
            vault.root().display(),
            err.message()
        ))
    })
}

fn signature_for(repo: &Repository) -> Signature<'static> {
    if let Ok(signature) = repo.signature() {
        return signature;
    }
    Signature::now("DayDoc", "daydoc@localhost")
        .unwrap_or_else(|_| Signature::now("DayDoc", "daydoc@localhost").expect("fallback signature"))
}

/// Files git sees as changed, including untracked ones.
pub fn status(vault: &Vault) -> Result<Vec<String>> {
    let repo = open_repo(vault)?;
    let mut options = git2::StatusOptions::new();
    options.include_untracked(true).recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut options)).map_err(git_err)?;
    let mut changed = Vec::new();
    for entry in statuses.iter() {
        if let Some(path) = entry.path() {
            changed.push(path.to_string());
        }
    }
    changed.sort();
    Ok(changed)
}

/// Stage everything and commit, if anything changed.
pub fn commit(vault: &Vault, message: Option<&str>) -> Result<SyncReport> {
    if vault.is_read_only() {
        return Err(DayDocError::ReadOnly("sync".to_string()));
    }
    let repo = open_repo(vault)?;
    let changed = status(vault)?;
    let message = message.unwrap_or(DEFAULT_MESSAGE).to_string();

    let mut index = repo.index().map_err(git_err)?;
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .map_err(git_err)?;
    index.write().map_err(git_err)?;
    let tree_id = index.write_tree().map_err(git_err)?;
    let tree = repo.find_tree(tree_id).map_err(git_err)?;

    let parent = match repo.head() {
        Ok(head) => Some(head.peel_to_commit().map_err(git_err)?),
        Err(_) => None,
    };

    if let Some(parent_commit) = &parent {
        if parent_commit.tree_id() == tree_id {
            return Ok(SyncReport {
                changed: Vec::new(),
                committed: false,
                message,
                head: Some(parent_commit.id().to_string()),
            });
        }
    }

    let signature = signature_for(&repo);
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    let oid = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            &message,
            &tree,
            &parents,
        )
        .map_err(git_err)?;

    // No audit line here on purpose: writing one after the commit would dirty
    // the tree again, and the git history is already the audit trail.
    Ok(SyncReport {
        changed,
        committed: true,
        message,
        head: Some(oid.to_string()),
    })
}

/// If a merge left conflict markers behind, keep both sides as a sidecar rather
/// than guessing. Returns the sidecars written.
pub fn quarantine_conflicts(vault: &Vault) -> Result<Vec<String>> {
    let mut written = Vec::new();
    for rel in vault.list_rels()? {
        let raw = match vault.read(&rel) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        if !raw.contains("<<<<<<<") || !raw.contains(">>>>>>>") {
            continue;
        }
        let sidecar = format!("{rel}.conflict.md");
        let mut body = String::from("# Conflict copy\n\n");
        body.push_str(&format!("DayDoc did not merge `{rel}`. Both sides are below.\n\n"));
        body.push_str("```\n");
        body.push_str(&raw);
        body.push_str("\n```\n");
        vault.write(&sidecar, &body)?;
        written.push(sidecar);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    fn init_repo(vault: &Vault) {
        Repository::init(vault.root()).expect("git init");
    }

    #[test]
    fn a_missing_repository_is_a_clear_error() {
        let vault = temp_vault();
        let err = commit(&vault, None).expect_err("should fail");
        assert!(err.to_string().contains("git init"));
    }

    #[test]
    fn commit_records_the_vault_and_a_second_run_is_a_no_op() {
        let vault = temp_vault();
        init_repo(&vault);
        vault
            .write("inbox/a.md", "---\nid: 01A\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("write");

        let report = commit(&vault, Some("first")).expect("commit");
        assert!(report.committed);
        assert!(report.head.is_some());
        assert!(report.changed.iter().any(|p| p.contains("inbox/a.md")));

        let again = commit(&vault, None).expect("commit again");
        assert!(!again.committed);
        assert!(again.changed.is_empty());
    }

    #[test]
    fn status_lists_untracked_files() {
        let vault = temp_vault();
        init_repo(&vault);
        vault
            .write("inbox/b.md", "---\nid: 01B\ntype: idea\n---\nbody\n")
            .expect("write");
        let changed = status(&vault).expect("status");
        assert!(changed.iter().any(|p| p.contains("inbox/b.md")));
    }

    #[test]
    fn conflict_markers_become_a_sidecar() {
        let vault = temp_vault();
        vault
            .write(
                "inbox/c.md",
                "---\nid: 01C\ntype: idea\n---\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> other\n",
            )
            .expect("write");
        let sidecars = quarantine_conflicts(&vault).expect("quarantine");
        assert_eq!(sidecars.len(), 1);
        assert!(sidecars[0].ends_with(".conflict.md"));
        let sidecar = vault.read(&sidecars[0]).expect("read sidecar");
        assert!(sidecar.contains("did not merge"));
    }
}
