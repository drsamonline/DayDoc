//! Calendar events, read-only, from an `.ics` file.
//!
//! The only thing a calendar does here is bias project tagging: an event called
//! "Atlas migration review" on the day three notes were written is a hint that
//! those notes belong to Atlas. DayDoc shows the hint and the user accepts it —
//! the calendar never writes anything by itself.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::error::{DayDocError, Result};
use crate::note::Note;
use crate::vault::{now_iso, Vault};

/// Words that say nothing about a project.
const STOPWORDS: [&str; 22] = [
    "the", "and", "for", "with", "from", "into", "about", "review", "meeting", "sync", "call",
    "weekly", "daily", "monthly", "standup", "stand", "update", "discussion", "catch", "follow",
    "kickoff", "status",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub summary: String,
    /// `YYYY-MM-DD`, when the event has a date we can read.
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagSuggestion {
    pub project: String,
    pub date: String,
    pub notes: Vec<String>,
}

impl TagSuggestion {
    pub fn render(&self) -> String {
        let mut out = format!("{} — {} note(s) on {}\n", self.project, self.notes.len(), self.date);
        for note in &self.notes {
            out.push_str(&format!("  - {note}\n"));
        }
        out
    }
}

/// Unfold RFC 5545 continuation lines, then keep the properties we use.
pub fn parse(text: &str) -> Vec<Event> {
    let mut unfolded: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = unfolded.last_mut() {
                last.push_str(line.trim_start());
                continue;
            }
        }
        unfolded.push(line.trim_end().to_string());
    }

    let mut events = Vec::new();
    let mut summary: Option<String> = None;
    let mut date: Option<String> = None;
    let mut inside = false;

    for line in unfolded {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("BEGIN:VEVENT") {
            inside = true;
            summary = None;
            date = None;
            continue;
        }
        if upper.starts_with("END:VEVENT") {
            if let Some(text) = summary.take() {
                events.push(Event { summary: text, date: date.take() });
            }
            inside = false;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some(value) = line.strip_prefix("SUMMARY:") {
            summary = Some(value.trim().to_string());
        } else if upper.starts_with("DTSTART") {
            if let Some(value) = line.split_once(':').map(|(_, value)| value.trim()) {
                if value.len() >= 8 && value[..8].chars().all(|c| c.is_ascii_digit()) {
                    date = Some(format!("{}-{}-{}", &value[..4], &value[4..6], &value[6..8]));
                }
            }
        }
    }
    events
}

pub fn load(path: &Path) -> Result<Vec<Event>> {
    let raw = std::fs::read_to_string(path).map_err(|e| DayDocError::io(path, e))?;
    Ok(parse(&raw))
}

fn tokens(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 4 && !STOPWORDS.contains(word))
        .map(|word| word.to_string())
        .collect()
}

fn note_date(vault: &Vault, note: &Note) -> Option<String> {
    for candidate in [
        note.frontmatter.created.as_deref(),
        note.frontmatter.updated.as_deref(),
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
    // A note written today with no date in its frontmatter is still from today.
    let path = vault.abs(&note.rel_path).ok()?;
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Local> = chrono::DateTime::from(modified);
    Some(datetime.format("%Y-%m-%d").to_string())
}

/// Notes written on an event's day that share a meaningful word with its title.
pub fn tag_suggestions(vault: &Vault, events: &[Event]) -> Result<Vec<TagSuggestion>> {
    let notes = vault.scan()?.notes;
    let mut suggestions: Vec<TagSuggestion> = Vec::new();
    let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();

    for event in events {
        let date = match &event.date {
            Some(date) => date.clone(),
            None => continue,
        };
        let event_tokens = tokens(&event.summary);
        if event_tokens.is_empty() {
            continue;
        }
        let mut matched: Vec<String> = Vec::new();
        for note in &notes {
            if note_date(vault, note).as_deref() != Some(date.as_str()) {
                continue;
            }
            let note_tokens = tokens(&format!("{} {}", note.title(), note.body));
            if event_tokens.iter().any(|token| note_tokens.contains(token)) {
                matched.push(note.rel_path.clone());
            }
        }
        if matched.is_empty() {
            continue;
        }
        let key = (event.summary.clone(), date.clone());
        if let Some(index) = seen.get(&key) {
            // The same event can appear twice; merge its notes.
            let existing = &mut suggestions[*index];
            for note in matched {
                if !existing.notes.contains(&note) {
                    existing.notes.push(note);
                }
            }
            continue;
        }
        seen.insert(key, suggestions.len());
        suggestions.push(TagSuggestion {
            project: event.summary.clone(),
            date,
            notes: matched,
        });
    }
    suggestions.sort_by(|a, b| b.notes.len().cmp(&a.notes.len()));
    Ok(suggestions)
}

/// Apply a suggestion: the project name goes into each note's frontmatter.
pub fn accept(vault: &Vault, suggestion: &TagSuggestion) -> Result<usize> {
    let mut tagged = 0usize;
    for rel in &suggestion.notes {
        let mut note = vault.load(rel)?;
        if note
            .frontmatter
            .projects
            .iter()
            .any(|project| project.eq_ignore_ascii_case(&suggestion.project))
        {
            continue;
        }
        note.frontmatter.projects.push(suggestion.project.clone());
        note.frontmatter.updated = Some(now_iso());
        vault.save(&note)?;
        tagged += 1;
    }
    if tagged > 0 {
        vault.audit(
            "calendar-tag",
            &suggestion.project,
            &format!("{tagged} note(s) tagged from an event title"),
        )?;
    }
    Ok(tagged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    const ICS: &str = "BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VEVENT\r
SUMMARY:Atlas migration review\r
DTSTART:20261120T090000Z\r
END:VEVENT\r
BEGIN:VEVENT\r
SUMMARY:Payroll filing deadline\r
DTSTART;VALUE=DATE:20261121\r
END:VEVENT\r
END:VCALENDAR\r
";

    #[test]
    fn events_are_read_with_their_dates() {
        let events = parse(ICS);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].summary, "Atlas migration review");
        assert_eq!(events[0].date.as_deref(), Some("2026-11-20"));
        assert_eq!(events[1].date.as_deref(), Some("2026-11-21"));
    }

    #[test]
    fn folded_lines_are_unfolded() {
        let folded = "BEGIN:VEVENT\nSUMMARY:Atlas migration\n  review\nDTSTART:20261120\nEND:VEVENT\n";
        let events = parse(folded);
        assert_eq!(events[0].summary, "Atlas migrationreview");
    }

    #[test]
    fn a_note_on_the_event_day_sharing_a_word_is_suggested() {
        let vault = temp_vault();
        vault
            .write(
                "meetings/atlas.md",
                "---\nid: 01A\ntype: meeting\ntitle: Atlas cutover plan\ncreated: 2026-11-20\n---\natlas migration notes\n",
            )
            .expect("note");
        vault
            .write(
                "meetings/other.md",
                "---\nid: 01B\ntype: meeting\ntitle: Payroll filing\ncreated: 2026-11-20\n---\nchallan\n",
            )
            .expect("other");
        let events = parse(ICS);
        let suggestions = tag_suggestions(&vault, &events).expect("suggest");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].project, "Atlas migration review");
        assert_eq!(suggestions[0].notes, vec!["meetings/atlas.md".to_string()]);
    }

    #[test]
    fn accepting_writes_the_project_and_is_idempotent() {
        let vault = temp_vault();
        vault
            .write(
                "meetings/atlas.md",
                "---\nid: 01A\ntype: meeting\ntitle: Atlas cutover\ncreated: 2026-11-20\n---\natlas migration\n",
            )
            .expect("note");
        let events = parse(ICS);
        let suggestion = tag_suggestions(&vault, &events).expect("suggest").remove(0);

        assert_eq!(accept(&vault, &suggestion).expect("accept"), 1);
        let note = vault.load("meetings/atlas.md").expect("load");
        assert_eq!(note.frontmatter.projects, vec!["Atlas migration review".to_string()]);

        // Second time: nothing to do.
        assert_eq!(accept(&vault, &suggestion).expect("accept"), 0);
    }

    #[test]
    fn generic_event_words_do_not_match() {
        let vault = temp_vault();
        vault
            .write(
                "meetings/m.md",
                "---\nid: 01A\ntype: meeting\ntitle: Weekly review\ncreated: 2026-11-20\n---\nweekly review of the week\n",
            )
            .expect("note");
        let events = parse("BEGIN:VEVENT\nSUMMARY:Weekly review\nDTSTART:20261120\nEND:VEVENT\n");
        assert!(tag_suggestions(&vault, &events).expect("suggest").is_empty());
    }
}
