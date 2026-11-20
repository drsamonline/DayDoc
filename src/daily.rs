//! The daily loop: morning plan, end-of-day wrap, Monday digest.

use chrono::Datelike;
use serde::Serialize;

use crate::error::Result;
use crate::ids;
use crate::ledger;
use crate::note::Note;
use crate::sop;
use crate::stale;
use crate::summarize::extractive_summary;
use crate::types::{DecisionStatus, NoteType};
use crate::vault::{now_iso, Vault};

/// Whether to offer the end-of-day wrap: after 17:00 local, and not already
/// written for today.
pub fn wrap_due(now: chrono::DateTime<chrono::Local>, already_written: bool) -> bool {
    use chrono::Timelike;
    !already_written && now.hour() >= 17
}

/// Has today's wrap been written into the daily note?
///
/// The wrap itself contains `##` sub-headings, so this reads the raw file rather
/// than asking for the `## End of day` section — that section is empty by
/// construction.
pub fn wrap_written(vault: &Vault, date: &str) -> bool {
    let raw = match vault.read(&daily_rel(date)) {
        Ok(raw) => raw,
        Err(_) => return false,
    };
    match raw.split_once("## End of day") {
        Some((_, rest)) => rest.trim().len() > 20,
        None => false,
    }
}

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

/// Structured end-of-day data. The markdown is a rendering of this, so a
/// script and a human see the same facts.
#[derive(Debug, Clone, Serialize)]
pub struct WrapData {
    pub date: String,
    pub decisions: Vec<String>,
    pub tasks: Vec<String>,
    pub touched: Vec<String>,
    pub summary: String,
}

impl WrapData {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# End of day — {}\n\n", self.date));

        out.push_str("## Decisions\n\n");
        if self.decisions.is_empty() {
            out.push_str("_None recorded today._\n\n");
        } else {
            for line in &self.decisions {
                out.push_str(&format!("- {line}\n"));
            }
            out.push('\n');
        }

        out.push_str("## Tasks opened\n\n");
        if self.tasks.is_empty() {
            out.push_str("_None._\n\n");
        } else {
            for task in &self.tasks {
                out.push_str(&format!("- [ ] {task}\n"));
            }
            out.push('\n');
        }

        out.push_str("## Notes touched\n\n");
        if self.touched.is_empty() {
            out.push_str("_None._\n\n");
        } else {
            for line in &self.touched {
                out.push_str(&format!("- {line}\n"));
            }
            out.push('\n');
        }

        out.push_str("## In your words\n\n");
        if self.summary.trim().is_empty() {
            out.push_str("_No prose captures today._\n\n");
        } else {
            out.push_str(&self.summary);
            out.push_str("\n\n");
        }
        out
    }
}

/// End-of-day wrap as data: structured counts plus an extractive summary of the
/// prose captures.
pub fn wrap_data(vault: &Vault, date: &str) -> Result<WrapData> {
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

    let joined = prose.join("\n");
    let summary = if joined.trim().is_empty() {
        String::new()
    } else {
        extractive_summary(&joined, 4)
    };

    Ok(WrapData {
        date: date.to_string(),
        decisions,
        tasks,
        touched,
        summary,
    })
}

/// End-of-day wrap as markdown.
pub fn eod_wrap(vault: &Vault, date: &str) -> Result<String> {
    Ok(wrap_data(vault, date)?.render())
}

/// Append the wrap into the daily note's `## End of day` section.
pub fn write_wrap(vault: &Vault, date: &str) -> Result<String> {
    let wrap = wrap_data(vault, date)?.render();
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

/// Structured weekly digest.
#[derive(Debug, Clone, Serialize)]
pub struct DigestData {
    pub week: String,
    pub from: String,
    pub to: String,
    pub decisions: Vec<String>,
    pub stale: Vec<String>,
    pub sop: Vec<String>,
    pub open_tasks: Vec<String>,
    pub old_inbox: Vec<String>,
}

impl DigestData {
    pub fn render(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!(
            "# Week {}\n\n_{} to {}_\n\n",
            self.week, self.from, self.to
        ));

        md.push_str("## Decisions made\n\n");
        if self.decisions.is_empty() {
            md.push_str("_None._\n\n");
        } else {
            for line in &self.decisions {
                md.push_str(&format!("- {line}\n"));
            }
            md.push('\n');
        }

        md.push_str("## Docs gone stale\n\n");
        if self.stale.is_empty() {
            md.push_str("_Nothing flagged._\n\n");
        } else {
            for line in &self.stale {
                md.push_str(&format!("- {line}\n"));
            }
            md.push('\n');
        }

        md.push_str("## SOP proposals\n\n");
        if self.sop.is_empty() {
            md.push_str("_None this week._\n\n");
        } else {
            for line in &self.sop {
                md.push_str(&format!("- {line}\n"));
            }
            md.push('\n');
        }

        md.push_str("## Open action items\n\n");
        if self.open_tasks.is_empty() {
            md.push_str("_None._\n\n");
        } else {
            for line in &self.open_tasks {
                md.push_str(&format!("- [ ] {line}\n"));
            }
            md.push('\n');
        }

        md.push_str("## Inbox older than 14 days\n\n");
        if self.old_inbox.is_empty() {
            md.push_str("_Nothing rotting._\n\n");
        } else {
            for line in &self.old_inbox {
                md.push_str(&format!("- {line}\n"));
            }
            md.push('\n');
        }
        md
    }
}

/// Monday digest as data: decisions, stale docs, SOP proposals, open action
/// items and old inbox items.
pub fn digest_data(vault: &Vault, date: &str) -> Result<(String, DigestData)> {
    let (from, to) = week_bounds(date).unwrap_or_else(|| (date.to_string(), date.to_string()));
    let label = iso_week(date);
    let notes = vault.scan()?.notes;

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

    let mut stale_docs: Vec<String> = stale::scan(vault)?
        .iter()
        .map(|flag| format!("{} ({})", flag.title, flag.rel_path))
        .collect();
    stale_docs.sort();

    let mut proposals: Vec<String> = sop::propose(vault)?
        .iter()
        .map(|proposal| {
            format!(
                "{} — {} notes describe the same sequence",
                proposal.title, proposal.support
            )
        })
        .collect();
    proposals.sort();

    let mut open: Vec<String> = Vec::new();
    for note in &notes {
        for task in open_task_lines(note) {
            open.push(format!("{task} _({})_", note.rel_path));
        }
    }
    open.truncate(40);

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

    let rel = format!("daily/digests/{label}.md");
    Ok((
        rel,
        DigestData {
            week: label,
            from,
            to,
            decisions,
            stale: stale_docs,
            sop: proposals,
            open_tasks: open,
            old_inbox,
        },
    ))
}

/// Weekly digest as markdown, plus the path it belongs at.
pub fn weekly_digest(vault: &Vault, date: &str) -> Result<(String, String)> {
    let (rel, data) = digest_data(vault, date)?;
    Ok((rel, data.render()))
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
    fn the_wrap_is_offered_after_five() {
        use chrono::TimeZone;
        let before = chrono::Local.with_ymd_and_hms(2026, 11, 20, 16, 59, 0).unwrap();
        let after = chrono::Local.with_ymd_and_hms(2026, 11, 20, 17, 0, 0).unwrap();
        assert!(!wrap_due(before, false));
        assert!(wrap_due(after, false));
        assert!(!wrap_due(after, true));
    }

    #[test]
    fn a_fresh_daily_note_has_no_wrap_and_a_written_one_does() {
        let vault = temp_vault();
        let date = today();
        assert!(!wrap_written(&vault, &date));
        ensure_daily(&vault, &date).expect("daily");
        assert!(!wrap_written(&vault, &date));
        write_wrap(&vault, &date).expect("wrap");
        assert!(wrap_written(&vault, &date));
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
