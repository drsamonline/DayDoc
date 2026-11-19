//! The handover packet: what I did, what is open, what is decided, who to ask,
//! and a link to the source for each. Markdown only at v1, plus the same facts
//! as structured data for a script.

use serde::Serialize;

use crate::error::Result;
use crate::ledger;
use crate::note::Note;
use crate::stale;
use crate::types::{DecisionStatus, NoteType};
use crate::vault::{now_iso, today, Vault};

/// Everything a handover says, before it is rendered.
#[derive(Debug, Clone, Serialize)]
pub struct HandoverData {
    pub from: String,
    pub to: String,
    /// "YYYY-MM-DD — title (type)"
    pub did: Vec<String>,
    /// Open task lines.
    pub open: Vec<String>,
    /// "DR-NNNN — title"
    pub decided: Vec<String>,
    /// (who, why) for every open thread.
    pub who: Vec<(String, String)>,
    /// "title (path)" for every stale document.
    pub stale: Vec<String>,
}

impl HandoverData {
    pub fn render(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!("# Handover {} to {}\n\n", self.from, self.to));
        md.push_str(&format!("_Generated {}. Markdown only._\n\n", now_iso()));

        md.push_str("## What I did\n\n");
        if self.did.is_empty() {
            md.push_str("_No notes touched in this period._\n\n");
        } else {
            for line in &self.did {
                md.push_str(&format!("- {line}\n"));
            }
            md.push('\n');
        }

        md.push_str("## What is open\n\n");
        if self.open.is_empty() {
            md.push_str("_No open tasks._\n\n");
        } else {
            for line in &self.open {
                md.push_str(&format!("- {line}\n"));
            }
            md.push('\n');
        }

        md.push_str("## What is decided\n\n");
        if self.decided.is_empty() {
            md.push_str("_No decisions recorded in this period._\n\n");
        } else {
            for line in &self.decided {
                md.push_str(&format!("- {line}\n"));
            }
            md.push('\n');
        }

        md.push_str("## Who to ask\n\n");
        if self.who.is_empty() {
            md.push_str("_Nobody has an open thread._\n\n");
        } else {
            for (who, why) in &self.who {
                md.push_str(&format!("- {who} — {why}\n"));
            }
            md.push('\n');
        }

        md.push_str("## Stale documents\n\n");
        if self.stale.is_empty() {
            md.push_str("_Nothing flagged._\n\n");
        } else {
            for line in &self.stale {
                md.push_str(&format!("- {line}\n"));
            }
            md.push('\n');
        }
        md
    }
}

#[derive(Debug, Clone)]
pub struct Handover {
    pub from: String,
    pub to: String,
    pub markdown: String,
}

/// A date on a note, in `YYYY-MM-DD`, from frontmatter or file mtime.
fn note_date(vault: &Vault, note: &Note) -> Option<String> {
    for candidate in [
        note.frontmatter.updated.as_deref(),
        note.frontmatter.created.as_deref(),
        note.frontmatter.decided_at.as_deref(),
        note.frontmatter.last_verified.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if candidate.len() >= 10 {
            return Some(candidate[..10].to_string());
        }
    }
    let path = vault.abs(&note.rel_path).ok()?;
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Local> = chrono::DateTime::from(modified);
    Some(datetime.format("%Y-%m-%d").to_string())
}

fn in_range(date: &str, from: &str, to: &str) -> bool {
    date >= from && date <= to
}

fn open_tasks(note: &Note) -> Vec<String> {
    note.body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let rest = trimmed
                .strip_prefix("- [ ]")
                .or_else(|| trimmed.strip_prefix("* [ ]"))?;
            let task = rest.trim();
            if task.is_empty() {
                None
            } else {
                Some(task.to_string())
            }
        })
        .collect()
}

fn person_in(task: &str) -> Option<String> {
    for token in task.split_whitespace() {
        if let Some(handle) = token.strip_prefix('@') {
            let cleaned =
                handle.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-');
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

/// Compile the packet's facts for a period, inclusive.
pub fn build_data(vault: &Vault, from: &str, to: &str) -> Result<HandoverData> {
    let notes = vault.scan()?.notes;
    let mut did: Vec<String> = Vec::new();
    let mut open: Vec<String> = Vec::new();
    let mut who: Vec<(String, String)> = Vec::new();

    for note in &notes {
        if note.note_type() == NoteType::Decision {
            continue;
        }
        let date = note_date(vault, note).unwrap_or_default();
        if in_range(&date, from, to) {
            did.push(format!("{date} — {} _({})_", note.title(), note.note_type()));
        }
        for task in open_tasks(note) {
            let owner = person_in(&task);
            if let Some(person) = &owner {
                who.push((person.clone(), format!("open task in {}", note.rel_path)));
            }
            match owner {
                Some(owner) => open.push(format!("{task} — {owner} _({})_", note.rel_path)),
                None => open.push(format!("{task} _({})_", note.rel_path)),
            }
        }
    }

    let mut decided: Vec<String> = Vec::new();
    for card in ledger::cards(vault)? {
        let date = card.decided_at.clone().unwrap_or_default();
        if card.status == DecisionStatus::Accepted && in_range(&date, from, to) {
            decided.push(format!("{} — {}", card.decision_id, card.title));
        }
        if card.status == DecisionStatus::Proposed {
            if let Some(owner) = &card.owner {
                who.push((
                    owner.clone(),
                    format!("owns proposed {}", card.decision_id),
                ));
            }
        }
    }

    let mut stale_docs: Vec<String> = Vec::new();
    for flag in stale::scan(vault)? {
        who.push((
            "documentation owner".to_string(),
            format!("{} is stale", flag.rel_path),
        ));
        stale_docs.push(format!("{} ({})", flag.title, flag.rel_path));
    }

    did.sort();
    did.dedup();
    decided.sort();
    who.sort();
    who.dedup();
    stale_docs.sort();

    Ok(HandoverData {
        from: from.to_string(),
        to: to.to_string(),
        did,
        open,
        decided,
        who,
        stale: stale_docs,
    })
}

/// Compile a handover packet for a period, inclusive.
pub fn build(vault: &Vault, from: &str, to: &str) -> Result<Handover> {
    let data = build_data(vault, from, to)?;
    Ok(Handover {
        from: data.from.clone(),
        to: data.to.clone(),
        markdown: data.render(),
    })
}

/// Write the packet into `handovers/`.
pub fn write(vault: &Vault, handover: &Handover) -> Result<String> {
    let rel = format!("handovers/{}.md", today());
    vault.write(&rel, &handover.markdown)?;
    vault.audit(
        "handover",
        &rel,
        &format!("period {} to {}", handover.from, handover.to),
    )?;
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::NewDecision;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn packet_has_every_section() {
        let vault = temp_vault();
        let date = today();
        ledger::create(
            &vault,
            &NewDecision::new("Use Okta for SSO")
                .accepted()
                .with_owner("alice"),
        )
        .expect("decision");
        vault
            .write(
                "projects/atlas.md",
                &format!(
                    "---\ntype: project-update\ntitle: Atlas status\nupdated: {date}\n---\n- [ ] ship the migration @priya\n"
                ),
            )
            .expect("note");

        let packet = build(&vault, &date, &date).expect("build");
        for heading in [
            "## What I did",
            "## What is open",
            "## What is decided",
            "## Who to ask",
            "## Stale documents",
        ] {
            assert!(packet.markdown.contains(heading), "missing {heading}");
        }
        assert!(packet.markdown.contains("Use Okta for SSO"));
        assert!(packet.markdown.contains("ship the migration"));
        assert!(packet.markdown.contains("priya"));
    }

    #[test]
    fn data_carries_the_same_facts_as_the_markdown() {
        let vault = temp_vault();
        let date = today();
        ledger::create(
            &vault,
            &NewDecision::new("Use Okta for SSO")
                .accepted()
                .with_owner("alice"),
        )
        .expect("decision");
        vault
            .write(
                "projects/atlas.md",
                &format!(
                    "---\ntype: project-update\ntitle: Atlas status\nupdated: {date}\n---\n- [ ] ship the migration @priya\n"
                ),
            )
            .expect("note");

        let data = build_data(&vault, &date, &date).expect("data");
        assert_eq!(data.from, date);
        assert!(data.did.iter().any(|line| line.contains("Atlas status")));
        assert!(data.open.iter().any(|line| line.contains("ship the migration")));
        assert!(data.decided.iter().any(|line| line.contains("DR-0001")));
        assert!(data.who.iter().any(|(who, _)| who == "priya"));
        // The rendered packet is exactly what the data says.
        let packet = build(&vault, &date, &date).expect("build");
        assert_eq!(packet.markdown, data.render());
    }

    #[test]
    fn writing_creates_a_file_under_handovers() {
        let vault = temp_vault();
        let packet = build(&vault, "2026-01-01", "2026-01-31").expect("build");
        let rel = write(&vault, &packet).expect("write");
        assert!(rel.starts_with("handovers/"));
        assert!(vault.exists(&rel));
    }
}
