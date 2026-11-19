//! Capture: one box, one keystroke. The classifier routes; low confidence goes
//! to `inbox/` with a type badge rather than guessing.

use crate::classify::{classify, Classification};
use crate::error::Result;
use crate::ids;
use crate::ledger::{self, NewDecision};
use crate::note::Note;
use crate::types::{DecisionStatus, NoteType};
use crate::vault::{now_iso, today, Vault};

#[derive(Debug, Clone)]
pub struct CaptureOutcome {
    pub rel_path: String,
    pub note_type: NoteType,
    pub confidence: f32,
    pub routed_to_inbox: bool,
}

impl CaptureOutcome {
    pub fn describe(&self) -> String {
        if self.routed_to_inbox {
            format!(
                "{}  [inbox, best guess {} at {:.0}% — one keystroke to reclassify]",
                self.rel_path,
                self.note_type,
                self.confidence * 100.0
            )
        } else {
            format!("{}  [{}]", self.rel_path, self.note_type)
        }
    }
}

fn title_from(text: &str) -> String {
    for line in text.lines() {
        let trimmed = line.trim().trim_start_matches('#').trim();
        let trimmed = trimmed.trim_start_matches("- ").trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut title = trimmed.to_string();
        if title.chars().count() > 80 {
            title = title.chars().take(77).collect::<String>() + "...";
        }
        return title;
    }
    "Untitled capture".to_string()
}

fn unique_path(vault: &Vault, folder: &str, date: &str, slug: &str) -> String {
    let mut candidate = format!("{folder}/{date}-{slug}.md");
    let mut counter = 2;
    while vault.exists(&candidate) {
        candidate = format!("{folder}/{date}-{slug}-{counter}.md");
        counter += 1;
    }
    candidate
}

/// Classify and store a capture.
pub fn capture(vault: &Vault, text: &str) -> Result<CaptureOutcome> {
    let classification = classify(text);
    let min = vault.config().min_classification_confidence;
    if classification.is_confident(min) {
        store(vault, text, classification.note_type, classification.confidence, false)
    } else {
        store(vault, text, NoteType::Inbox, classification.confidence, true)
    }
}

/// Store a capture as a specific type, bypassing the classifier.
pub fn capture_as(vault: &Vault, text: &str, note_type: NoteType) -> Result<CaptureOutcome> {
    store(vault, text, note_type, 1.0, false)
}

/// What the classifier thinks, without writing anything.
pub fn preview(text: &str) -> Classification {
    classify(text)
}

fn store(
    vault: &Vault,
    text: &str,
    note_type: NoteType,
    confidence: f32,
    routed_to_inbox: bool,
) -> Result<CaptureOutcome> {
    let title = title_from(text);
    let date = today();
    let slug = ids::slug(&title, 50);

    if note_type == NoteType::Decision {
        // A decision is titled by what was decided, not by the announcement.
        let first_line = text
            .lines()
            .map(|line| line.trim().trim_start_matches('#').trim())
            .find(|line| !line.is_empty())
            .unwrap_or("");
        let decision_title = crate::meeting::decision_title(first_line);
        let mut request =
            NewDecision::new(decision_title).with_decision(text.trim().to_string());
        request.status = DecisionStatus::Proposed;
        let note = ledger::create(vault, &request)?;
        return Ok(CaptureOutcome {
            rel_path: note.rel_path.clone(),
            note_type,
            confidence,
            routed_to_inbox: false,
        });
    }

    let folder = note_type.folder();
    let rel = unique_path(vault, folder, &date, &slug);
    let mut note = Note::parse(&rel, "---\n---\n")?;
    note.frontmatter.id = ids::new_ulid();
    note.frontmatter.note_type = note_type.as_str().to_string();
    note.frontmatter.title = title;
    note.frontmatter.created = Some(now_iso());
    note.frontmatter.updated = Some(now_iso());
    note.frontmatter.last_verified = Some(date);
    note.body = text.trim().to_string();
    vault.create(&note.rel_path, &note.render())?;
    vault.audit("capture", &note.rel_path, note_type.as_str())?;

    Ok(CaptureOutcome {
        rel_path: note.rel_path,
        note_type,
        confidence,
        routed_to_inbox,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn a_clear_decision_becomes_a_proposed_decision() {
        let vault = temp_vault();
        let outcome = capture(&vault, "We decided to adopt Okta for SSO across all products.").expect("capture");
        assert_eq!(outcome.note_type, NoteType::Decision);
        assert!(!outcome.routed_to_inbox);
        let note = vault.load(&outcome.rel_path).expect("load");
        assert_eq!(note.frontmatter.status.as_deref(), Some("proposed"));
        assert!(note.frontmatter.decision_id.is_some());
    }

    #[test]
    fn ambiguous_text_goes_to_inbox_with_a_badge() {
        let vault = temp_vault();
        let outcome = capture(&vault, "hmm").expect("capture");
        assert!(outcome.routed_to_inbox);
        assert!(outcome.rel_path.starts_with("inbox/"));
        assert!(outcome.describe().contains("inbox"));
    }

    #[test]
    fn a_checklist_lands_in_tasks() {
        let vault = temp_vault();
        let outcome = capture(&vault, "- [ ] send the invoice\n- [ ] book the room\n").expect("capture");
        assert_eq!(outcome.note_type, NoteType::Task);
        assert!(outcome.rel_path.starts_with("tasks/"));
    }

    #[test]
    fn same_title_same_day_does_not_overwrite() {
        let vault = temp_vault();
        let first = capture_as(&vault, "note body one", NoteType::Idea).expect("first");
        let second = capture_as(&vault, "note body one", NoteType::Idea).expect("second");
        assert_ne!(first.rel_path, second.rel_path);
        assert!(vault.exists(&second.rel_path));
    }

    #[test]
    fn read_only_vault_refuses_capture() {
        let mut vault = temp_vault();
        vault.config_mut().read_only_vault = true;
        assert!(capture(&vault, "we decided to ship on friday").is_err());
    }
}
