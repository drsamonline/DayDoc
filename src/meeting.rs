//! Meeting → decision pipeline, Tier 0.
//!
//! Paste a transcript, get one-line candidate cards. Nothing is written until
//! a human approves a candidate: extraction proposes, the ledger records.

use std::sync::OnceLock;

use regex::Regex;

use crate::classify::decision_score;
use crate::error::Result;
use crate::ids;
use crate::ledger::{self, NewDecision};
use crate::note::Note;
use crate::types::DecisionStatus;
use crate::vault::Vault;

/// A decision-shaped sentence, ready to be approved as a `DR-NNNN`.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub sentence: String,
    pub title: String,
    pub score: f32,
    pub owner: Option<String>,
    /// A `DR-NNNN` the sentence says it replaces, if any.
    pub supersedes: Option<String>,
}

impl Candidate {
    /// `Adopt Okta for SSO — approve as DR-0012?`
    pub fn card(&self, next_id: &str) -> String {
        let mut out = format!("{} — approve as {next_id}?", self.title);
        if let Some(owner) = &self.owner {
            out.push_str(&format!("  [owner: {owner}]"));
        }
        if let Some(old) = &self.supersedes {
            out.push_str(&format!("  [replaces {old}]"));
        }
        out
    }
}

fn sentence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)([^.!?\n]+[.!?]?)").expect("sentence regex"))
}

fn supersede_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:replaces?|supersedes?|instead of)\s+(DR-\d{1,6})").expect("supersede regex")
    })
}

fn owner_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:owner|owned by|assigned to|action for)\s*[:\-]?\s*@?([A-Z][A-Za-z'\-]{1,30})")
            .expect("owner regex")
    })
}

fn handle_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"@([A-Za-z][A-Za-z0-9_.\-]{1,30})").expect("handle regex"))
}

fn will_own_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b([A-Z][A-Za-z'\-]{1,30})\s+will\s+(?:own|drive|lead|take|handle|send|prepare)\b")
            .expect("will-own regex")
    })
}

fn speaker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\s*(?:[A-Z][A-Za-z'\-\.]{0,29}|[A-Z][A-Za-z'\-\. ]{0,29})\s*:\s*")
            .expect("speaker regex")
    })
}

fn action_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^\s*(?:[-*]\s*\[\s?\]|action(?:\s*item)?\s*[:\-]|todo\s*[:\-]|ai\s*[:\-])\s*(.+)$")
            .expect("action regex")
    })
}

/// Split into sentences, keeping line boundaries as hard stops.
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Bullets are already atomic.
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with('#') {
            out.push(trimmed.to_string());
            continue;
        }
        let spoken = speaker_re().replace(trimmed, "");
        let spoken = spoken.trim();
        for capture in sentence_re().captures_iter(spoken) {
            if let Some(m) = capture.get(1) {
                let sentence = m.as_str().trim();
                if !sentence.is_empty() {
                    out.push(sentence.to_string());
                }
            }
        }
    }
    out
}

/// Strip the cue phrase so the title is the decision, not the announcement.
fn title_from(sentence: &str) -> String {
    let mut text = sentence.trim().to_string();
    let lowered = text.to_ascii_lowercase();
    const PREFIXES: [&str; 14] = [
        "we decided to ",
        "we decided ",
        "we have decided to ",
        "let's go with ",
        "lets go with ",
        "final call is ",
        "the final call is ",
        "agreed to ",
        "we agreed to ",
        "we're going with ",
        "we are going with ",
        "decision: ",
        "decision is ",
        "we will ",
    ];
    for prefix in PREFIXES {
        if lowered.starts_with(prefix) {
            text = text[prefix.len()..].trim().to_string();
            break;
        }
    }
    // "final call is to move billing" -> "move billing"
    if text.to_ascii_lowercase().starts_with("to ") {
        text = text[3..].trim().to_string();
    }
    text = text.trim_end_matches(['.', '!', ';']).trim().to_string();
    let mut chars = text.chars();
    let capitalised = match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    let mut out = capitalised;
    if out.len() > 90 {
        let cut = out
            .char_indices()
            .take_while(|(i, _)| *i <= 87)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        out.truncate(cut);
        out.push_str("...");
    }
    if out.trim().is_empty() {
        "Untitled decision".to_string()
    } else {
        out
    }
}

/// Title for a decision: the cue phrase stripped, the object kept.
///
/// `we decided to adopt Okta for SSO` becomes `Adopt Okta for SSO`.
pub fn decision_title(sentence: &str) -> String {
    title_from(sentence)
}

/// Best owner guess from the sentence, then from the surrounding text.
fn owner_from(sentence: &str) -> Option<String> {
    for re in [owner_re(), will_own_re()] {
        if let Some(caps) = re.captures(sentence) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().trim().to_string());
            }
        }
    }
    if let Some(caps) = handle_re().captures(sentence) {
        if let Some(m) = caps.get(1) {
            return Some(m.as_str().trim().to_string());
        }
    }
    None
}

/// Extract decision candidates from a transcript or meeting note.
pub fn extract_decisions(text: &str) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = Vec::new();
    for sentence in sentences(text) {
        let score = decision_score(&sentence);
        if score < 2.0 {
            continue;
        }
        let supersedes = supersede_re()
            .captures(&sentence)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_ascii_uppercase()));
        let candidate = Candidate {
            title: title_from(&sentence),
            owner: owner_from(&sentence),
            supersedes,
            score,
            sentence: sentence.clone(),
        };
        // Same title twice in one transcript is one decision.
        if !candidates
            .iter()
            .any(|c| c.title.eq_ignore_ascii_case(&candidate.title))
        {
            candidates.push(candidate);
        }
    }
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    candidates
}

/// Action items in a transcript: explicit checkboxes or `action:` lines.
pub fn extract_action_items(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    for line in text.lines() {
        if let Some(caps) = action_re().captures(line) {
            if let Some(m) = caps.get(1) {
                let item = m.as_str().trim().trim_end_matches('.').to_string();
                if !item.is_empty() && !items.contains(&item) {
                    items.push(item);
                }
            }
        }
    }
    items
}

/// The next reference a candidate would take, for the preview card.
pub fn next_reference(vault: &Vault) -> String {
    ledger::all(vault)
        .map(|notes| {
            let highest = notes
                .iter()
                .filter_map(|n| n.frontmatter.decision_id.as_deref())
                .filter_map(ids::parse_decision_id)
                .max()
                .unwrap_or(0);
            ids::decision_id(highest + 1)
        })
        .unwrap_or_else(|_| "DR-0001".to_string())
}

/// Approve candidates: write real decision notes, link the source, update the
/// supersession chain. Returns the notes written, in approval order.
pub fn approve(vault: &Vault, source_rel: &str, candidates: &[Candidate]) -> Result<Vec<Note>> {
    let mut written = Vec::new();
    for candidate in candidates {
        let mut request = NewDecision::new(candidate.title.clone())
            .with_decision(candidate.sentence.clone())
            .with_source(source_rel);
        request.status = DecisionStatus::Accepted;
        request.owner = candidate.owner.clone();
        request.supersedes = candidate.supersedes.clone();
        request.context = format!("Extracted from {source_rel}.");
        let note = ledger::create(vault, &request)?;
        written.push(note);
    }
    vault.audit(
        "meeting-approval",
        source_rel,
        &format!("{} decision(s) approved", written.len()),
    )?;
    Ok(written)
}

/// Write approved action items into the source note as tasks.
pub fn append_action_items(vault: &Vault, source_rel: &str, items: &[String]) -> Result<usize> {
    if items.is_empty() {
        return Ok(0);
    }
    let mut note = vault.load(source_rel)?;
    if !note.body.contains("## Action items") {
        note.body.push_str("\n## Action items\n\n");
    }
    let mut added = 0;
    for item in items {
        let line = format!("- [ ] {item}");
        if !note.body.contains(&line) {
            note.body.push_str(&line);
            note.body.push('\n');
            added += 1;
        }
    }
    vault.save(&note)?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    const TRANSCRIPT: &str = "\
Alice: we need SSO before the enterprise pilot.
Bob: we decided to adopt Okta for SSO across all products.
Carol: final call is to move billing to Razorpay instead of Stripe.
Alice: let's go with a 30 day pilot, owner: Priya
- [ ] Priya will prepare the rollout checklist
action: Bob to send the security questionnaire
";

    #[test]
    fn extracts_three_candidates_not_noise() {
        let candidates = extract_decisions(TRANSCRIPT);
        let titles: Vec<String> = candidates.iter().map(|c| c.title.clone()).collect();
        assert!(titles.iter().any(|t| t.contains("Adopt Okta")), "{titles:?}");
        assert!(titles.iter().any(|t| t.contains("Move billing to Razorpay")), "{titles:?}");
        assert!(titles.iter().any(|t| t.contains("30 day pilot")), "{titles:?}");
        assert!(!titles.iter().any(|t| t.contains("we need SSO")));
    }

    #[test]
    fn owner_is_prefilled() {
        let candidates = extract_decisions(TRANSCRIPT);
        let pilot = candidates
            .iter()
            .find(|c| c.title.contains("30 day pilot"))
            .expect("pilot candidate");
        assert_eq!(pilot.owner.as_deref(), Some("Priya"));
    }

    #[test]
    fn action_items_are_extracted() {
        let items = extract_action_items(TRANSCRIPT);
        assert!(items.iter().any(|i| i.contains("rollout checklist")), "{items:?}");
        assert!(items.iter().any(|i| i.contains("security questionnaire")), "{items:?}");
    }

    #[test]
    fn approval_writes_decisions_and_links_the_source() {
        let vault = temp_vault();
        vault
            .write("meetings/kickoff.md", &format!("---\ntype: meeting\ntitle: Kickoff\n---\n{TRANSCRIPT}"))
            .expect("write");
        let candidates = extract_decisions(TRANSCRIPT);
        let picked: Vec<Candidate> = candidates
            .into_iter()
            .filter(|c| c.title.contains("Okta"))
            .collect();
        let written = approve(&vault, "meetings/kickoff.md", &picked).expect("approve");
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].frontmatter.source.as_deref(), Some("meetings/kickoff.md"));
        assert_eq!(
            written[0].frontmatter.status.as_deref(),
            Some("accepted")
        );
        assert!(vault.exists("decisions/DR-0001.md"));
    }

    #[test]
    fn cards_read_like_a_question() {
        let candidate = Candidate {
            sentence: "we decided to adopt Okta".into(),
            title: "Adopt Okta".into(),
            score: 3.0,
            owner: Some("Alice".into()),
            supersedes: None,
        };
        assert_eq!(
            candidate.card("DR-0012"),
            "Adopt Okta — approve as DR-0012?  [owner: Alice]"
        );
    }

    #[test]
    fn supersedes_hint_is_detected() {
        let text = "we decided to use Entra instead of DR-0003 for SSO.";
        let candidates = extract_decisions(text);
        assert_eq!(candidates[0].supersedes.as_deref(), Some("DR-0003"));
    }
}
