//! The handover packet: what I did, what is open, what is decided, who to ask,
//! and a link to the source for each. Markdown only at v1.

use crate::error::Result;
use crate::ledger;
use crate::note::Note;
use crate::stale;
use crate::types::{DecisionStatus, NoteType};
use crate::vault::{now_iso, today, Vault};

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
            let cleaned = handle.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-');
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
}

/// Compile a handover packet for a period, inclusive.
pub fn build(vault: &Vault, from: &str, to: &str) -> Result<Handover> {
    let notes = vault.scan()?.notes;
    let mut touched: Vec<(String, String, NoteType)> = Vec::new();
    let mut tasks: Vec<(String, String, Option<String>)> = Vec::new();
    let mut people: Vec<(String, String)> = Vec::new();

    for note in &notes {
        if note.note_type() == NoteType::Decision {
            continue;
        }
        let date = note_date(vault, note).unwrap_or_default();
        if in_range(&date, from, to) {
            touched.push((date.clone(), note.title(), note.note_type()));
        }
        for task in open_tasks(note) {
            let owner = person_in(&task);
            if let Some(who) = &owner {
                people.push((who.clone(), format!("open task in {}", note.rel_path)));
            }
            tasks.push((note.title(), task, owner));
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
                people.push((
                    owner.clone(),
                    format!("owns proposed {}", card.decision_id),
                ));
            }
        }
    }

    let stale_flags = stale::scan(vault)?;
    for flag in &stale_flags {
        people.push((
            "documentation owner".to_string(),
            format!("{} is stale", flag.rel_path),
        ));
    }

    touched.sort();
    touched.dedup_by(|a, b| a.1 == b.1);

    let mut md = String::new();
    md.push_str(&format!("# Handover {from} to {to}\n\n"));
    md.push_str(&format!("_Generated {}. Markdown only._\n\n", now_iso()));

    md.push_str("## What I did\n\n");
    if touched.is_empty() {
        md.push_str("_No notes touched in this period._\n\n");
    } else {
        for (date, title, note_type) in &touched {
            md.push_str(&format!("- {date} — {title} _({note_type})_\n"));
        }
        md.push('\n');
    }

    md.push_str("## What is open\n\n");
    if tasks.is_empty() {
        md.push_str("_No open tasks._\n\n");
    } else {
        for (source, task, owner) in &tasks {
            match owner {
                Some(owner) => md.push_str(&format!("- {task} — {owner} _({source})_\n")),
                None => md.push_str(&format!("- {task} _({source})_\n")),
            }
        }
        md.push('\n');
    }

    md.push_str("## What is decided\n\n");
    if decided.is_empty() {
        md.push_str("_No decisions recorded in this period._\n\n");
    } else {
        for line in &decided {
            md.push_str(&format!("- {line}\n"));
        }
        md.push('\n');
    }

    md.push_str("## Who to ask\n\n");
    people.sort();
    people.dedup();
    if people.is_empty() {
        md.push_str("_Nobody has an open thread._\n\n");
    } else {
        for (who, why) in &people {
            md.push_str(&format!("- {who} — {why}\n"));
        }
        md.push('\n');
    }

    md.push_str("## Stale documents\n\n");
    if stale_flags.is_empty() {
        md.push_str("_Nothing flagged._\n\n");
    } else {
        for flag in &stale_flags {
            md.push_str(&format!("- {} ({})\n", flag.title, flag.rel_path));
            for reason in &flag.reasons {
                md.push_str(&format!("  - {}\n", reason.describe()));
            }
        }
        md.push('\n');
    }

    Ok(Handover {
        from: from.to_string(),
        to: to.to_string(),
        markdown: md,
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
            &NewDecision::new("Use Okta for SSO").accepted().with_owner("alice"),
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
    fn writing_creates_a_file_under_handovers() {
        let vault = temp_vault();
        let packet = build(&vault, "2026-01-01", "2026-01-31").expect("build");
        let rel = write(&vault, &packet).expect("write");
        assert!(rel.starts_with("handovers/"));
        assert!(vault.exists(&rel));
    }
}
