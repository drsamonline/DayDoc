//! Intent classification, Tier 0: no model, no network, no download.
//!
//! A weighted cue table plus structural signals (checkboxes, attendee blocks,
//! ordered steps). The score is a margin over the runner-up, which is what
//! makes a two-way tie land in `inbox/` for the user to reclassify in one
//! keystroke instead of guessing.

use std::collections::BTreeMap;

use crate::types::NoteType;

#[derive(Debug, Clone)]
pub struct Classification {
    pub note_type: NoteType,
    pub confidence: f32,
    pub scores: BTreeMap<&'static str, f32>,
}

impl Classification {
    pub fn is_confident(&self, min_confidence: f32) -> bool {
        self.confidence >= min_confidence && self.note_type != NoteType::Inbox
    }

    pub fn badge(&self) -> String {
        format!("{} ({:.0}%)", self.note_type, self.confidence * 100.0)
    }
}

fn cues(note_type: NoteType) -> &'static [(&'static str, f32)] {
    match note_type {
        NoteType::Decision => &[
            ("we decided", 3.0),
            ("decided to", 2.5),
            ("decision:", 3.0),
            ("decision is", 2.5),
            ("final call", 3.0),
            ("let's go with", 2.8),
            ("agreed to", 2.0),
            ("we're going with", 2.8),
            ("sign off", 2.0),
            ("approved", 1.5),
            ("supersedes", 2.5),
            ("going forward we", 2.0),
        ],
        NoteType::Meeting => &[
            ("attendees", 3.0),
            ("participants", 2.5),
            ("agenda", 3.0),
            ("minutes", 2.5),
            ("meeting", 2.0),
            ("standup", 2.2),
            ("sync with", 1.5),
            ("call with", 1.5),
            ("action items", 1.5),
            ("next steps", 1.5),
        ],
        NoteType::Task => &[
            ("todo", 3.0),
            ("to-do", 3.0),
            ("action item", 2.5),
            ("deadline", 2.0),
            ("due by", 2.0),
            ("follow up", 2.0),
            ("assigned to", 2.0),
            ("remind me", 2.2),
            ("remember to", 2.2),
        ],
        NoteType::Idea => &[
            ("what if", 2.5),
            ("idea:", 3.0),
            ("brainstorm", 3.0),
            ("maybe we could", 2.5),
            ("someday", 2.0),
            ("concept", 1.5),
            ("worth exploring", 2.2),
        ],
        NoteType::Sop => &[
            ("step 1", 3.0),
            ("procedure", 3.0),
            ("runbook", 3.0),
            ("checklist", 2.5),
            ("sop", 3.0),
            ("how to", 2.2),
            ("first, then", 1.5),
            ("process:", 2.0),
        ],
        NoteType::ProjectUpdate => &[
            ("status:", 3.0),
            ("on track", 3.0),
            ("blocked", 2.5),
            ("milestone", 2.0),
            ("update:", 2.5),
            ("progress", 2.0),
            ("slipping", 2.2),
            ("at risk", 2.2),
        ],
        NoteType::Reference => &[
            ("http://", 2.5),
            ("https://", 2.5),
            ("reference", 2.0),
            ("docs:", 2.0),
            ("link:", 1.8),
            ("summary of", 2.2),
            ("quote", 1.5),
        ],
        NoteType::Daily => &[
            ("today i", 2.5),
            ("this morning", 2.5),
            ("end of day", 3.0),
            ("daily", 2.0),
            ("what i did", 2.5),
            ("tomorrow i", 2.0),
        ],
        NoteType::Inbox => &[],
    }
}

fn structural(note_type: NoteType, lowered: &str) -> f32 {
    let mut score = 0.0;
    match note_type {
        NoteType::Task => {
            for line in lowered.lines() {
                let t = line.trim_start();
                if t.starts_with("- [ ]") || t.starts_with("* [ ]") || t.starts_with("[ ]") {
                    score += 3.0;
                }
            }
        }
        NoteType::Meeting => {
            if lowered.contains("## attendees") || lowered.contains("## participants") {
                score += 4.0;
            }
            if lowered.contains("## agenda") {
                score += 3.0;
            }
        }
        NoteType::Sop => {
            let mut numbered = 0;
            for line in lowered.lines() {
                let t = line.trim_start();
                let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() && (t[digits.len()..].starts_with(". ") || t[digits.len()..].starts_with(") ")) {
                    numbered += 1;
                }
            }
            if numbered >= 3 {
                score += 3.0;
            }
        }
        NoteType::Decision => {
            if lowered.contains("dr-") && lowered.contains("supersede") {
                score += 2.0;
            }
        }
        _ => {}
    }
    score
}

/// Classify a capture. Always returns an answer; low confidence is the signal
/// to route to `inbox/`, not a reason to fail.
pub fn classify(text: &str) -> Classification {
    let lowered = text.to_ascii_lowercase();
    let mut scores: BTreeMap<&'static str, f32> = BTreeMap::new();

    for note_type in NoteType::ALL {
        if note_type == NoteType::Inbox {
            continue;
        }
        let mut score = structural(note_type, &lowered);
        for (cue, weight) in cues(note_type) {
            if lowered.contains(cue) {
                score += weight;
            }
        }
        if score > 0.0 {
            scores.insert(note_type.as_str(), score);
        }
    }

    if scores.is_empty() {
        return Classification {
            note_type: NoteType::Inbox,
            confidence: 0.0,
            scores,
        };
    }

    let mut ranked: Vec<(&'static str, f32)> = scores.iter().map(|(k, v)| (*k, *v)).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let (top_key, top_score) = ranked[0];
    let second = ranked.get(1).map(|(_, s)| *s).unwrap_or(0.0);

    let margin = if top_score > 0.0 {
        (top_score - second) / top_score
    } else {
        0.0
    };
    let confidence = (0.5 + 0.5 * margin).clamp(0.0, 1.0);
    let note_type = NoteType::parse(top_key).unwrap_or(NoteType::Inbox);

    // A single weak cue is not a classification.
    if top_score < 1.5 {
        return Classification {
            note_type: NoteType::Inbox,
            confidence: confidence * 0.5,
            scores,
        };
    }

    Classification {
        note_type,
        confidence,
        scores,
    }
}

/// Does this sentence read as a decision? Used by the meeting pipeline.
pub fn decision_score(sentence: &str) -> f32 {
    let lowered = sentence.to_ascii_lowercase();
    let mut score = 0.0;
    for (cue, weight) in cues(NoteType::Decision) {
        if lowered.contains(cue) {
            score += weight;
        }
    }
    // A decision names a thing. Bare "we decided" with no object is not one.
    let words = lowered.split_whitespace().count();
    if words < 4 {
        score *= 0.5;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clear_decision_is_confident() {
        let c = classify("We decided to adopt Okta for SSO across all products.");
        assert_eq!(c.note_type, NoteType::Decision);
        assert!(c.is_confident(0.7), "confidence {}", c.confidence);
    }

    #[test]
    fn a_checklist_is_a_task() {
        let c = classify("- [ ] send the invoice\n- [ ] book the room\n");
        assert_eq!(c.note_type, NoteType::Task);
    }

    #[test]
    fn an_attendee_block_is_a_meeting() {
        let c = classify("## Attendees\nAlice, Bob\n\n## Agenda\npricing");
        assert_eq!(c.note_type, NoteType::Meeting);
    }

    #[test]
    fn three_numbered_steps_are_an_sop() {
        let c = classify("1. open the console\n2. rotate the key\n3. restart the worker\n");
        assert_eq!(c.note_type, NoteType::Sop);
    }

    #[test]
    fn ambiguous_text_lands_in_inbox() {
        let c = classify("hmm");
        assert_eq!(c.note_type, NoteType::Inbox);
        assert!(!c.is_confident(0.7));
    }

    #[test]
    fn decision_score_ignores_empty_phrases() {
        assert!(decision_score("We decided to adopt Okta for SSO") > 2.0);
        assert!(decision_score("we decided") < 2.0);
    }
}
