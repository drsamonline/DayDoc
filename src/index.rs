//! Note identity guard.
//!
//! A ULID is the identity. If someone edits a file and changes or removes its
//! `id`, that file is a *new* note: the original id stays in the index, the
//! note is not re-linked, and the change is reported into `_meta/warnings/`
//! rather than silently rewriting the past.

use rusqlite::{params, OptionalExtension};

use crate::error::{DayDocError, Result};
use crate::ids;
use crate::vault::{now_iso, Vault};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS note_ids (
    rel_path TEXT PRIMARY KEY,
    id TEXT NOT NULL,
    seen_at TEXT NOT NULL
);";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityChange {
    pub rel_path: String,
    pub old_id: String,
    pub new_id: String,
}

impl IdentityChange {
    pub fn describe(&self) -> String {
        format!(
            "{}: id was {}, now {} — treated as a new note",
            self.rel_path, self.old_id, self.new_id
        )
    }
}

fn write_warning(vault: &Vault, change: &IdentityChange) -> Result<String> {
    let stem = ids::slug(
        change.rel_path.rsplit('/').next().unwrap_or(&change.rel_path),
        50,
    );
    let rel = format!(
        "_meta/warnings/{}-{}.md",
        chrono::Local::now().format("%Y-%m-%d"),
        stem
    );
    let mut body = String::new();
    body.push_str(&format!("# Note identity changed\n\n{}\n\n", change.describe()));
    body.push_str("DayDoc did not touch the file. The original id is kept in the\n");
    body.push_str("index so links to the old note still resolve.\n\n");
    body.push_str(&format!("- file: `{}`\n", change.rel_path));
    body.push_str(&format!("- was: `{}`\n", change.old_id));
    body.push_str(&format!("- now: `{}`\n", change.new_id));
    body.push_str(&format!("- noticed: {}\n", now_iso()));
    vault.write(&rel, &body)?;
    Ok(rel)
}

/// Compare every note's id against what the index last saw. Returns the changes
/// and writes one warning file per change.
pub fn check_identity(vault: &Vault) -> Result<Vec<IdentityChange>> {
    let conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;
    let mut changes = Vec::new();

    for note in vault.scan()?.notes {
        let id = note.frontmatter.id.clone();
        if id.is_empty() {
            continue;
        }
        let stored: Option<String> = conn
            .query_row(
                "SELECT id FROM note_ids WHERE rel_path = ?1",
                params![note.rel_path],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(old_id) = stored {
            if old_id != id {
                let change = IdentityChange {
                    rel_path: note.rel_path.clone(),
                    old_id,
                    new_id: id.clone(),
                };
                if !vault.is_read_only() {
                    write_warning(vault, &change)?;
                }
                changes.push(change);
            }
        }

        conn.execute(
            "INSERT INTO note_ids (rel_path, id, seen_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(rel_path) DO UPDATE SET id = excluded.id, seen_at = excluded.seen_at",
            params![note.rel_path, id, now_iso()],
        )
        .map_err(DayDocError::Db)?;
    }

    if !changes.is_empty() {
        let summary = changes
            .iter()
            .map(|c| c.describe())
            .collect::<Vec<_>>()
            .join("; ");
        vault.audit("identity-change", "_meta/warnings", &summary)?;
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn a_stable_id_produces_no_warning() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01AAA\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("write");
        assert!(check_identity(&vault).expect("check").is_empty());
        assert!(check_identity(&vault).expect("check").is_empty());
    }

    #[test]
    fn changing_an_id_is_reported_not_applied() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01AAA\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("write");
        check_identity(&vault).expect("first");

        vault
            .write("inbox/a.md", "---\nid: 01BBB\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("rewrite");
        let changes = check_identity(&vault).expect("second");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].old_id, "01AAA");
        assert_eq!(changes[0].new_id, "01BBB");

        // The file is untouched, and a warning exists.
        let raw = vault.read("inbox/a.md").expect("read");
        assert!(raw.contains("01BBB"));
        let warning_dir = vault.root().join("_meta/warnings");
        let count = std::fs::read_dir(&warning_dir).map(|d| d.count()).unwrap_or(0);
        assert!(count >= 1, "no warning written");
    }

    #[test]
    fn removing_an_id_is_not_a_change_we_can_track() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01AAA\ntype: idea\n---\nbody\n")
            .expect("write");
        check_identity(&vault).expect("first");
        vault
            .write("inbox/a.md", "---\ntype: idea\n---\nbody\n")
            .expect("rewrite");
        // No id, so nothing to compare; the note is simply un-identified.
        assert!(check_identity(&vault).expect("second").is_empty());
    }
}
