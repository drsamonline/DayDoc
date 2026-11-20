//! Vault health: the checks that catch a vault quietly rotting.
//!
//! Lint never writes. It reports, and the user decides — the same posture as
//! stale detection.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::error::Result;
use crate::ids;
use crate::ledger;
use crate::note::Note;
use crate::vault::Vault;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueKind {
    MissingId,
    DuplicateId,
    UnknownType,
    UnknownDecisionRef,
    BrokenLink,
    MissingAsset,
    OrphanAsset,
}

impl IssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::MissingId => "missing-id",
            IssueKind::DuplicateId => "duplicate-id",
            IssueKind::UnknownType => "unknown-type",
            IssueKind::UnknownDecisionRef => "unknown-decision-ref",
            IssueKind::BrokenLink => "broken-link",
            IssueKind::MissingAsset => "missing-asset",
            IssueKind::OrphanAsset => "orphan-asset",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub kind: IssueKind,
    pub rel_path: String,
    pub detail: String,
}

impl Issue {
    pub fn render(&self) -> String {
        format!("[{}] {} — {}\n", self.kind.as_str(), self.rel_path, self.detail)
    }
}

fn cited_decisions(note: &Note) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for token in note
        .search_text()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
    {
        let upper = token.to_ascii_uppercase();
        if upper.starts_with("DR-")
            && ids::parse_decision_id(&upper).is_some()
            && !found.contains(&upper)
        {
            found.push(upper);
        }
    }
    found
}

/// `assets/...` paths mentioned in a note body.
fn asset_refs(note: &Note) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let body = &note.body;
    let mut index = 0usize;
    while let Some(found_at) = body[index..].find("assets/") {
        let start = index + found_at;
        let rest = &body[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '"' | ']' | '>' | '#' | '?'))
            .unwrap_or(rest.len());
        let candidate = rest[..end].trim_end_matches(['.', ',', ';']).to_string();
        if candidate.len() > "assets/".len() && !found.contains(&candidate) {
            found.push(candidate);
        }
        index = start + "assets/".len();
    }
    found
}

fn asset_files(vault: &Vault) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let root = vault.root().join("assets");
    if !root.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => stack.push(path),
                Ok(meta) if meta.is_file() => out.push(vault.rel_of(&path)),
                _ => {}
            }
        }
    }
    out.sort();
    Ok(out)
}

/// What `lint --fix` changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixReport {
    pub assigned_ids: usize,
    pub files: Vec<String>,
}

impl FixReport {
    pub fn render(&self) -> String {
        if self.assigned_ids == 0 {
            return "nothing to fix\n".to_string();
        }
        let mut out = format!("assigned {} missing id(s)\n", self.assigned_ids);
        for file in &self.files {
            out.push_str(&format!("  {file}\n"));
        }
        out
    }
}

/// The one fix lint is allowed to make: a note with no `id` gets a fresh ULID.
///
/// Nothing else is repaired automatically. A duplicate id, an unknown decision
/// reference or an orphaned attachment are judgement calls, and lint reports
/// them instead of guessing.
pub fn fix(vault: &Vault) -> Result<FixReport> {
    let mut report = FixReport::default();
    for note in vault.scan()?.notes {
        if !note.frontmatter.id.trim().is_empty() {
            continue;
        }
        let mut note = note;
        note.frontmatter.id = ids::new_ulid();
        note.frontmatter.updated = Some(crate::vault::now_iso());
        vault.save(&note)?;
        report.assigned_ids += 1;
        report.files.push(note.rel_path.clone());
    }
    if report.assigned_ids > 0 {
        vault.audit(
            "lint-fix",
            &report.assigned_ids.to_string(),
            "assigned missing note ids",
        )?;
    }
    Ok(report)
}

/// Every problem lint can see, in one pass over the vault.
pub fn scan(vault: &Vault) -> Result<Vec<Issue>> {
    let notes = vault.scan()?.notes;
    let known: BTreeSet<String> = ledger::all(vault)?
        .iter()
        .filter_map(|note| note.frontmatter.decision_id.clone())
        .collect();

    let mut issues = Vec::new();
    let mut ids: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut referenced: BTreeSet<String> = BTreeSet::new();

    for note in &notes {
        let rel = note.rel_path.clone();
        let id = note.frontmatter.id.trim().to_string();
        if id.is_empty() {
            issues.push(Issue {
                kind: IssueKind::MissingId,
                rel_path: rel.clone(),
                detail: "no id in frontmatter".to_string(),
            });
        } else {
            ids.entry(id).or_default().push(rel.clone());
        }

        if note.frontmatter.note_type().is_none() {
            issues.push(Issue {
                kind: IssueKind::UnknownType,
                rel_path: rel.clone(),
                detail: format!(
                    "type \"{}\" is not one of the nine note types",
                    note.frontmatter.note_type
                ),
            });
        }

        for link in &note.frontmatter.links {
            if !vault.exists(link) {
                issues.push(Issue {
                    kind: IssueKind::BrokenLink,
                    rel_path: rel.clone(),
                    detail: format!("links to {link}, which is not in the vault"),
                });
            }
        }

        for decision in cited_decisions(note) {
            if !known.contains(&decision) {
                issues.push(Issue {
                    kind: IssueKind::UnknownDecisionRef,
                    rel_path: rel.clone(),
                    detail: format!("cites {decision}, which is not in the ledger"),
                });
            }
        }

        for asset in asset_refs(note) {
            referenced.insert(asset.clone());
            if !vault.exists(&asset) {
                issues.push(Issue {
                    kind: IssueKind::MissingAsset,
                    rel_path: rel.clone(),
                    detail: format!("references {asset}, which is not on disk"),
                });
            }
        }
    }

    for (id, rels) in ids {
        if rels.len() < 2 {
            continue;
        }
        for rel in rels {
            issues.push(Issue {
                kind: IssueKind::DuplicateId,
                rel_path: rel,
                detail: format!("id {id} is used by more than one note"),
            });
        }
    }

    for asset in asset_files(vault)? {
        if !referenced.contains(&asset) {
            issues.push(Issue {
                kind: IssueKind::OrphanAsset,
                rel_path: asset.clone(),
                detail: "no note references this file".to_string(),
            });
        }
    }

    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn a_clean_vault_has_no_issues() {
        let vault = temp_vault();
        vault
            .write(
                "inbox/a.md",
                "---\nid: 01AAA\ntype: idea\ntitle: A\n---\nbody\n",
            )
            .expect("write");
        assert!(scan(&vault).expect("scan").is_empty());
    }

    #[test]
    fn missing_and_duplicate_ids_are_reported() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01AAA\ntype: idea\n---\nbody\n")
            .expect("a");
        vault
            .write("inbox/b.md", "---\nid: 01AAA\ntype: idea\n---\nbody\n")
            .expect("b");
        vault
            .write("inbox/c.md", "---\ntype: idea\n---\nbody\n")
            .expect("c");
        let issues = scan(&vault).expect("scan");
        assert!(issues.iter().any(|i| i.kind == IssueKind::DuplicateId));
        assert!(issues.iter().any(|i| i.kind == IssueKind::MissingId));
    }

    #[test]
    fn unknown_decision_references_are_reported() {
        let vault = temp_vault();
        vault
            .write(
                "meetings/m.md",
                "---\nid: 01BBB\ntype: meeting\ntitle: M\n---\nwe confirmed DR-0404.\n",
            )
            .expect("write");
        let issues = scan(&vault).expect("scan");
        assert!(issues
            .iter()
            .any(|i| i.kind == IssueKind::UnknownDecisionRef));
    }

    #[test]
    fn broken_links_and_assets_are_reported() {
        let vault = temp_vault();
        vault
            .write(
                "inbox/a.md",
                "---\nid: 01AAA\ntype: idea\nlinks: [inbox/gone.md]\n---\n![x](../assets/ab/missing.png)\n",
            )
            .expect("write");
        vault
            .write("assets/cd/orphan.png", "bytes")
            .expect("asset");
        let issues = scan(&vault).expect("scan");
        assert!(issues.iter().any(|i| i.kind == IssueKind::BrokenLink));
        assert!(issues.iter().any(|i| i.kind == IssueKind::MissingAsset));
        assert!(issues.iter().any(|i| i.kind == IssueKind::OrphanAsset));
    }

    #[test]
    fn fix_assigns_a_missing_id_and_leaves_the_rest_alone() {
        let vault = temp_vault();
        vault
            .write("inbox/noid.md", "---\ntype: idea\ntitle: No id\n---\nbody\n")
            .expect("write");
        vault
            .write("inbox/ok.md", "---\nid: 01KEEP\ntype: idea\ntitle: Fine\n---\nbody\n")
            .expect("write");

        let report = fix(&vault).expect("fix");
        assert_eq!(report.assigned_ids, 1);
        assert_eq!(report.files, vec!["inbox/noid.md".to_string()]);
        assert!(report.render().contains("assigned 1"));

        let fixed = vault.load("inbox/noid.md").expect("reload");
        assert_eq!(fixed.frontmatter.id.len(), 26);
        let untouched = vault.load("inbox/ok.md").expect("reload");
        assert_eq!(untouched.frontmatter.id, "01KEEP");

        assert!(scan(&vault)
            .expect("scan")
            .iter()
            .all(|issue| issue.kind != IssueKind::MissingId));

        // A second pass has nothing left to do.
        assert_eq!(fix(&vault).expect("fix again").assigned_ids, 0);
    }

    #[test]
    fn unknown_types_are_reported() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01AAA\ntype: nonsense\n---\nbody\n")
            .expect("write");
        let issues = scan(&vault).expect("scan");
        assert!(issues.iter().any(|i| i.kind == IssueKind::UnknownType));
    }
}
