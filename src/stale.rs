//! Stale detection: two independent signals.
//!
//! Age by note type (configurable), and contradiction — a note that cites a
//! superseded decision, an owner who has left, or a project that was renamed.
//! A flag always comes with three ways out: re-verify, edit, archive.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::ids;
use crate::note::Note;
use crate::types::{DecisionStatus, NoteType};
use crate::vault::{now_iso, today, Vault};

pub const DEPARTED_ROSTER: &str = "_meta/people/departed.md";
pub const RENAMED_ROSTER: &str = "_meta/projects/renamed.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleReason {
    Age { days: i64, threshold: i64 },
    CitesSuperseded { decision: String, superseded_by: String },
    OwnerDeparted { owner: String },
    ProjectRenamed { from: String, to: String },
}

impl StaleReason {
    pub fn describe(&self) -> String {
        match self {
            StaleReason::Age { days, threshold } => {
                format!("{days} days old (threshold {threshold})")
            }
            StaleReason::CitesSuperseded {
                decision,
                superseded_by,
            } => format!("cites {decision}, superseded by {superseded_by}"),
            StaleReason::OwnerDeparted { owner } => format!("owner {owner} has left"),
            StaleReason::ProjectRenamed { from, to } => format!("project {from} is now {to}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleFlag {
    pub rel_path: String,
    pub title: String,
    pub note_type: NoteType,
    pub reasons: Vec<StaleReason>,
}

impl StaleFlag {
    pub fn render(&self) -> String {
        let mut out = format!("{}  [{}]\n", self.title, self.note_type);
        out.push_str(&format!("  {}\n", self.rel_path));
        for reason in &self.reasons {
            out.push_str(&format!("  - {}\n", reason.describe()));
        }
        out
    }
}

fn parse_date(raw: &str) -> Option<chrono::NaiveDate> {
    let trimmed = raw.trim();
    let head = trimmed.split('T').next().unwrap_or(trimmed);
    chrono::NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()
}

fn age_days(note: &Note) -> Option<i64> {
    let candidates = [
        note.frontmatter.last_verified.as_deref(),
        note.frontmatter.updated.as_deref(),
        note.frontmatter.created.as_deref(),
        note.frontmatter.decided_at.as_deref(),
    ];
    for candidate in candidates.into_iter().flatten() {
        if let Some(date) = parse_date(candidate) {
            let today = chrono::Local::now().date_naive();
            return Some((today - date).num_days());
        }
    }
    None
}

/// Lines of a roster file, `- name` or `old -> new`.
fn roster(vault: &Vault, rel: &str) -> Vec<String> {
    vault
        .read(rel)
        .map(|raw| {
            raw.lines()
                .map(|l| l.trim().trim_start_matches('-').trim().to_string())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect()
        })
        .unwrap_or_default()
}

fn decision_statuses(vault: &Vault) -> BTreeMap<String, (DecisionStatus, Option<String>)> {
    let mut map = BTreeMap::new();
    if let Ok(notes) = crate::ledger::all(vault) {
        for note in notes {
            if let Some(id) = note.frontmatter.decision_id.clone() {
                map.insert(
                    id,
                    (
                        crate::ledger::status_of(&note),
                        note.frontmatter.superseded_by.clone(),
                    ),
                );
            }
        }
    }
    map
}

fn cited_decisions(note: &Note) -> Vec<String> {
    let mut found = Vec::new();
    for token in note
        .search_text()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
    {
        let upper = token.to_ascii_uppercase();
        if upper.starts_with("DR-") && ids::parse_decision_id(&upper).is_some() && !found.contains(&upper) {
            found.push(upper);
        }
    }
    found
}

/// Scan the vault for stale notes.
pub fn scan(vault: &Vault) -> Result<Vec<StaleFlag>> {
    let statuses = decision_statuses(vault);
    let departed = roster(vault, DEPARTED_ROSTER);
    let renamed: Vec<(String, String)> = roster(vault, RENAMED_ROSTER)
        .iter()
        .filter_map(|line| {
            let (from, to) = line.split_once("->")?;
            Some((from.trim().to_string(), to.trim().to_string()))
        })
        .collect();

    let mut flags = Vec::new();
    for note in vault.scan()?.notes {
        let mut reasons = Vec::new();
        let note_type = note.note_type();
        let status = crate::ledger::status_of(&note);

        // Signal one: age by type.
        if let Some(days) = age_days(&note) {
            let threshold = if note_type == NoteType::Decision {
                if status == DecisionStatus::Proposed {
                    Some(vault.config().stale_thresholds.decision_proposed)
                } else {
                    // An accepted decision never goes stale; it gets superseded.
                    None
                }
            } else {
                note_type
                    .stale_key()
                    .and_then(|key| vault.config().stale_thresholds.for_key(key))
            };
            if let Some(threshold) = threshold {
                if days > threshold {
                    reasons.push(StaleReason::Age { days, threshold });
                }
            }
        }

        // Signal two: contradiction.
        if note_type != NoteType::Decision {
            for decision in cited_decisions(&note) {
                if let Some((DecisionStatus::Superseded, Some(by))) = statuses.get(&decision) {
                    reasons.push(StaleReason::CitesSuperseded {
                        decision: decision.clone(),
                        superseded_by: by.clone(),
                    });
                }
            }
        }
        if let Some(owner) = &note.frontmatter.owner {
            if departed.iter().any(|d| d.eq_ignore_ascii_case(owner)) {
                reasons.push(StaleReason::OwnerDeparted {
                    owner: owner.clone(),
                });
            }
        }
        for project in &note.frontmatter.projects {
            if let Some((_, to)) = renamed.iter().find(|(from, _)| from.eq_ignore_ascii_case(project)) {
                reasons.push(StaleReason::ProjectRenamed {
                    from: project.clone(),
                    to: to.clone(),
                });
            }
        }

        if !reasons.is_empty() {
            flags.push(StaleFlag {
                rel_path: note.rel_path.clone(),
                title: note.title(),
                note_type,
                reasons,
            });
        }
    }
    Ok(flags)
}

/// Re-verify: the note is still true. Clears the flag, stamps the date.
pub fn reverify(vault: &Vault, rel: &str) -> Result<Note> {
    let mut note = vault.load(rel)?;
    note.frontmatter.last_verified = Some(today());
    note.frontmatter.updated = Some(now_iso());
    note.frontmatter.stale = None;
    vault.save(&note)?;
    vault.audit("reverified", rel, "last_verified updated")?;
    Ok(note)
}

/// Edit: remove a stale reference from the body without touching anything else.
pub fn remove_reference(vault: &Vault, rel: &str, needle: &str) -> Result<Note> {
    let mut note = vault.load(rel)?;
    let before = note.body.clone();
    note.body = note
        .body
        .lines()
        .filter(|line| !line.contains(needle))
        .collect::<Vec<_>>()
        .join("\n");
    note.body.push('\n');
    note.frontmatter.updated = Some(now_iso());
    vault.save(&note)?;
    vault.audit(
        "stale-reference-removed",
        rel,
        &format!("removed lines containing {needle} ({} chars before)", before.len()),
    )?;
    Ok(note)
}

/// Archive: move the note out of the live vault.
pub fn archive(vault: &Vault, rel: &str) -> Result<String> {
    vault.archive(rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{self, NewDecision};
    use crate::vault::test_support::temp_vault;

    fn old_date(days: i64) -> String {
        let date = chrono::Local::now().date_naive() - chrono::Duration::days(days);
        date.format("%Y-%m-%d").to_string()
    }

    #[test]
    fn old_reference_notes_are_flagged_by_age() {
        let vault = temp_vault();
        vault
            .write(
                "reference/old.md",
                &format!("---\ntype: reference\ntitle: Old doc\nlast_verified: {}\n---\nbody\n", old_date(400)),
            )
            .expect("write");
        let flags = scan(&vault).expect("scan");
        assert_eq!(flags.len(), 1);
        assert!(matches!(flags[0].reasons[0], StaleReason::Age { .. }));
    }

    #[test]
    fn accepted_decisions_never_go_stale() {
        let vault = temp_vault();
        ledger::create(
            &vault,
            &NewDecision::new("Use Okta for SSO")
                .accepted()
                .with_decision("ok"),
        )
        .expect("create");
        let mut note = vault.load("decisions/DR-0001.md").expect("load");
        note.frontmatter.decided_at = Some(old_date(2000));
        note.frontmatter.last_verified = Some(old_date(2000));
        vault.save(&note).expect("save");
        let flags = scan(&vault).expect("scan");
        assert!(flags.is_empty(), "accepted decision was flagged: {flags:?}");
    }

    #[test]
    fn proposed_decisions_do_go_stale() {
        let vault = temp_vault();
        ledger::create(&vault, &NewDecision::new("Maybe move to Entra")).expect("create");
        let mut note = vault.load("decisions/DR-0001.md").expect("load");
        note.frontmatter.decided_at = Some(old_date(200));
        note.frontmatter.last_verified = Some(old_date(200));
        note.frontmatter.created = Some(old_date(200));
        vault.save(&note).expect("save");
        let flags = scan(&vault).expect("scan");
        assert_eq!(flags.len(), 1);
    }

    #[test]
    fn citing_a_superseded_decision_is_a_contradiction() {
        let vault = temp_vault();
        ledger::create(&vault, &NewDecision::new("Use Okta for SSO").accepted()).expect("a");
        ledger::supersede(
            &vault,
            "DR-0001",
            &NewDecision::new("Use Entra ID for SSO"),
        )
        .expect("b");
        vault
            .write(
                "meetings/old.md",
                &format!(
                    "---\ntype: meeting\ntitle: Old sync\nlast_verified: {}\n---\nWe confirmed DR-0001.\n",
                    today()
                ),
            )
            .expect("write");
        let flags = scan(&vault).expect("scan");
        let meeting = flags
            .iter()
            .find(|f| f.rel_path == "meetings/old.md")
            .expect("meeting flagged");
        assert!(meeting
            .reasons
            .iter()
            .any(|r| matches!(r, StaleReason::CitesSuperseded { .. })));
    }

    #[test]
    fn departed_owner_and_renamed_project_are_contradictions() {
        let vault = temp_vault();
        vault
            .write("_meta/people/departed.md", "- Priya\n")
            .expect("roster");
        vault
            .write("_meta/projects/renamed.md", "- Atlas -> Atlas Prime\n")
            .expect("roster");
        vault
            .write(
                "projects/atlas.md",
                &format!(
                    "---\ntype: project-update\ntitle: Atlas status\nowner: Priya\nprojects: [Atlas]\nlast_verified: {}\n---\non track\n",
                    today()
                ),
            )
            .expect("write");
        let flags = scan(&vault).expect("scan");
        let reasons = &flags[0].reasons;
        assert!(reasons.iter().any(|r| matches!(r, StaleReason::OwnerDeparted { .. })));
        assert!(reasons.iter().any(|r| matches!(r, StaleReason::ProjectRenamed { .. })));
    }

    #[test]
    fn reverify_clears_the_age_flag() {
        let vault = temp_vault();
        vault
            .write(
                "reference/old.md",
                &format!("---\ntype: reference\ntitle: Old\nlast_verified: {}\n---\nbody\n", old_date(400)),
            )
            .expect("write");
        assert_eq!(scan(&vault).expect("scan").len(), 1);
        reverify(&vault, "reference/old.md").expect("reverify");
        assert!(scan(&vault).expect("scan").is_empty());
    }

    #[test]
    fn remove_reference_drops_only_the_matching_line() {
        let vault = temp_vault();
        vault
            .write(
                "meetings/m.md",
                &format!("---\ntype: meeting\ntitle: M\nlast_verified: {}\n---\nline one\ncites DR-0009 here\nline three\n", today()),
            )
            .expect("write");
        let note = remove_reference(&vault, "meetings/m.md", "DR-0009").expect("edit");
        assert!(!note.body.contains("DR-0009"));
        assert!(note.body.contains("line one"));
        assert!(note.body.contains("line three"));
    }
}
