//! The daily loop: morning plan, end-of-day wrap, Monday digest.

use chrono::Datelike;

use crate::error::Result;
use crate::ids;
use crate::ledger;
use crate::note::Note;
use crate::sop;
use crate::stale;
use crate::summarize::extractive_summary;
use crate::types::{DecisionStatus, NoteType};
use crate::vault::{now_iso, Vault};

pub fn daily_rel(date: &str) -> String {
    format!("daily/{date}.md")
}

/// Create today's daily note if it does not exist, and return it.
pub fn ensure_daily(vault: &Vault, date: &str) -> Result<Note> {
    let rel = daily_rel(date);
    if vault.exists(&rel) {
        return vault.load(&rel);
    }
    let mut note = Note::parse(&rel, "---\n---\n")?;
    note.frontmatter.id = ids::new_ulid();
    note.frontmatter.note_type = NoteType::Daily.as_str().to_string();
    note.frontmatter.title = format!("Daily — {date}");
    note.frontmatter.created = Some(now_iso());
    note.frontmatter.updated = Some(now_iso());
    note.frontmatter.last_verified = Some(date.to_string());
    note.body = "## Plan\n\n_What are you working on today?_\n\n## Captures\n\n## End of day\n".to_string();
    vault.create(&note.rel_path, &note.render())?;
    Ok(note)
}

fn date_of(note: &Note) -> String {
    for candidate in [
        note.frontmatter.created.as_deref(),
        note.frontmatter.updated.as_deref(),
        note.frontmatter.decided_at.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if candidate.len() >= 10 {
            return candidate[..10].to_string();
        }
    }
    String::new()
}

fn open_task_lines(note: &Note) -> Vec<String> {
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

/// End-of-day wrap: structured counts plus an extractive summary of the prose
/// captures, in a form the user can edit in thirty seconds.
pub fn eod_wrap(vault: &Vault, date: &str) -> Result<String> {
    let notes = vault.scan()?.notes;
    let cards = ledger::cards(vault)?;

    let mut decisions: Vec<String> = cards
        .iter()
        .filter(|c| c.decided_at.as_deref() == Some(date))
        .map(|c| format!("{} — {} ({})", c.decision_id, c.title, c.status))
        .collect();
    decisions.sort();

    let mut touched: Vec<String> = Vec::new();
    let mut tasks: Vec<String> = Vec::new();
    let mut prose: Vec<String> = Vec::new();
    for note in &notes {
        if note.note_type() == NoteType::Decision {
            continue;
        }
        if date_of(note) != date {
            continue;
        }
        touched.push(format!("{} ({})", note.title(), note.rel_path));
        for task in open_task_lines(note) {
            tasks.push(task);
        }
        if matches!(
            note.note_type(),
            NoteType::Inbox | NoteType::Idea | NoteType::Daily
        ) {
            prose.push(note.body.clone());
        }
    }
    touched.sort();
    touched.dedup();

    let mut out = String::new();
    out.push_str(&format!("# End of day — {date}\n\n"));

    out.push_str("## Decisions\n\n");
    if decisions.is_empty() {
        out.push_str("_None recorded today._\n\n");
    } else {
        for line in &decisions {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Tasks opened\n\n");
    if tasks.is_empty() {
        out.push_str("_None._\n\n");
    } else {
        for task in &tasks {
            out.push_str(&format!("- [ ] {task}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Notes touched\n\n");
    if touched.is_empty() {
        out.push_str("_None._\n\n");
    } else {
        for line in &touched {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }

    out.push_str("## In your words\n\n");
    let joined = prose.join("\n");
    if joined.trim().is_empty() {
        out.push_str("_No prose captures today._\n\n");
    } else {
        out.push_str(&extractive_summary(&joined, 4));
        out.push_str("\n\n");
    }
    Ok(out)
}

/// Append the wrap into the daily note's `## End of day` section.
pub fn write_wrap(vault: &Vault, date: &str) -> Result<String> {
    let wrap = eod_wrap(vault, date)?;
    let mut note = ensure_daily(vault, date)?;
    let header = format!("# End of day — {date}");
    let section = wrap.replacen(&header, "", 1).trim().to_string();

    let mut body = note.body.clone();
    if let Some(pos) = body.find("## End of day") {
        body.truncate(pos);
    }
    body.push_str("## End of day\n\n");
    body.push_str(&section);
    body.push('\n');
    note.body = body;
    note.frontmatter.updated = Some(now_iso());
    vault.save(&note)?;
    Ok(note.rel_path)
}

fn week_bounds(date: &str) -> Option<(String, String)> {
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let offset = parsed.weekday().num_days_from_monday() as i64;
    let monday = parsed - chrono::Duration::days(offset);
    let sunday = monday + chrono::Duration::days(6);
    Some((
        monday.format("%Y-%m-%d").to_string(),
        sunday.format("%Y-%m-%d").to_string(),
    ))
}

/// `2026-W12` style label for the ISO week containing `date`.
pub fn iso_week(date: &str) -> String {
    match chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(parsed) => parsed.format("%G-W%V").to_string(),
        Err(_) => date.to_string(),
    }
}

/// Monday digest: decisions, stale docs, SOP proposals, open action items and
/// old inbox items.
pub fn weekly_digest(vault: &Vault, date: &str) -> Result<(String, String)> {
    let (from, to) = week_bounds(date).unwrap_or_else(|| (date.to_string(), date.to_string()));
    let label = iso_week(date);
    let notes = vault.scan()?.notes;

    let mut md = String::new();
    md.push_str(&format!("# Week {label}\n\n_{from} to {to}_\n\n"));

    md.push_str("## Decisions made\n\n");
    let mut decisions: Vec<String> = ledger::cards(vault)?
        .iter()
        .filter(|c| {
            c.status == DecisionStatus::Accepted
                && c.decided_at
                    .as_deref()
                    .map(|d| d >= from.as_str() && d <= to.as_str())
                    .unwrap_or(false)
        })
        .map(|c| format!("{} — {}", c.decision_id, c.title))
        .collect();
    decisions.sort();
    if decisions.is_empty() {
        md.push_str("_None._\n\n");
    } else {
        for line in &decisions {
            md.push_str(&format!("- {line}\n"));
        }
        md.push('\n');
    }

    md.push_str("## Docs gone stale\n\n");
    let flags = stale::scan(vault)?;
    if flags.is_empty() {
        md.push_str("_Nothing flagged._\n\n");
    } else {
        for flag in &flags {
            md.push_str(&format!("- {} ({})\n", flag.title, flag.rel_path));
        }
        md.push('\n');
    }

    md.push_str("## SOP proposals\n\n");
    let proposals = sop::propose(vault)?;
    if proposals.is_empty() {
        md.push_str("_None this week._\n\n");
    } else {
        for proposal in &proposals {
            md.push_str(&format!(
                "- {} — {} notes describe the same sequence\n",
                proposal.title, proposal.support
            ));
        }
        md.push('\n');
    }

    md.push_str("## Open action items\n\n");
    let mut open: Vec<String> = Vec::new();
    for note in &notes {
        for task in open_task_lines(note) {
            open.push(format!("{task} _({})_", note.rel_path));
        }
    }
    if open.is_empty() {
        md.push_str("_None._\n\n");
    } else {
        for line in open.iter().take(40) {
            md.push_str(&format!("- [ ] {line}\n"));
        }
        md.push('\n');
    }

    md.push_str("## Inbox older than 14 days\n\n");
    let cutoff = (chrono::Local::now().date_naive() - chrono::Duration::days(14))
        .format("%Y-%m-%d")
        .to_string();
    let mut old_inbox: Vec<String> = notes
        .iter()
        .filter(|n| n.rel_path.starts_with("inbox/"))
        .filter(|n| {
            let d = date_of(n);
            !d.is_empty() && d < cutoff
        })
        .map(|n| format!("{} ({})", n.title(), n.rel_path))
        .collect();
    old_inbox.sort();
    if old_inbox.is_empty() {
        md.push_str("_Nothing rotting._\n\n");
    } else {
        for line in &old_inbox {
            md.push_str(&format!("- {line}\n"));
        }
        md.push('\n');
    }

    let rel = format!("daily/digests/{label}.md");
    Ok((rel, md))
}

/// Write the digest to `daily/digests/`.
pub fn write_digest(vault: &Vault, date: &str) -> Result<String> {
    let (rel, md) = weekly_digest(vault, date)?;
    vault.write(&rel, &md)?;
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::NewDecision;
    use crate::vault::today;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn daily_note_is_created_once() {
        let vault = temp_vault();
        let date = today();
        let first = ensure_daily(&vault, &date).expect("first");
        let second = ensure_daily(&vault, &date).expect("second");
        assert_eq!(first.rel_path, second.rel_path);
        assert_eq!(first.rel_path, format!("daily/{date}.md"));
        assert!(vault.exists(&format!("daily/{date}.md")));
    }

    #[test]
    fn wrap_counts_todays_decisions_and_tasks() {
        let vault = temp_vault();
        let date = today();
        ledger::create(&vault, &NewDecision::new("Use Okta for SSO").accepted()).expect("decision");
        crate::capture::capture(&vault, "- [ ] send the invoice\n- [ ] book the room\n").expect("capture");
        let wrap = eod_wrap(&vault, &date).expect("wrap");
        assert!(wrap.contains("DR-0001"));
        assert!(wrap.contains("send the invoice"));
    }

    #[test]
    fn iso_week_is_stable() {
        assert_eq!(iso_week("2026-03-04"), "2026-W10");
        assert_eq!(iso_week("2026-01-01"), "2026-W01");
    }

    #[test]
    fn digest_has_every_section() {
        let vault = temp_vault();
        let date = today();
        let (rel, md) = weekly_digest(&vault, &date).expect("digest");
        assert!(rel.starts_with("daily/digests/"));
        for heading in [
            "## Decisions made",
            "## Docs gone stale",
            "## SOP proposals",
            "## Open action items",
            "## Inbox older than 14 days",
        ] {
            assert!(md.contains(heading), "missing {heading}");
        }
    }
}
