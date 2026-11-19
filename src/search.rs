//! Search and time-anchored answers.
//!
//! Results are decision cards, not prose. `daydoc ask "what did we decide about
//! okta"` returns the decision, its status, its owner, what it replaced and
//! what it is cited by — not a list of paragraphs to re-read.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::error::Result;
use crate::ledger::{self, DecisionCard};
use crate::note::Note;
use crate::types::{DecisionStatus, NoteType};
use crate::vault::Vault;

const STOPWORDS: [&str; 24] = [
    "the", "a", "an", "and", "or", "of", "to", "in", "on", "for", "is", "are", "was", "were",
    "did", "do", "does", "we", "us", "about", "what", "who", "when", "why",
];

#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub note_type: Option<NoteType>,
    pub person: Option<String>,
    pub project: Option<String>,
    pub status: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
}

impl Filters {
    pub fn is_empty(&self) -> bool {
        self.note_type.is_none()
            && self.person.is_none()
            && self.project.is_none()
            && self.status.is_none()
            && self.since.is_none()
            && self.until.is_none()
    }

    fn matches(&self, note: &Note) -> bool {
        if let Some(note_type) = self.note_type {
            if note.note_type() != note_type {
                return false;
            }
        }
        if let Some(person) = &self.person {
            let needle = person.to_ascii_lowercase();
            let hit = note
                .frontmatter
                .people
                .iter()
                .chain(note.frontmatter.owner.iter())
                .any(|p| p.to_ascii_lowercase() == needle);
            if !hit {
                return false;
            }
        }
        if let Some(project) = &self.project {
            let needle = project.to_ascii_lowercase();
            if !note
                .frontmatter
                .projects
                .iter()
                .any(|p| p.to_ascii_lowercase() == needle)
            {
                return false;
            }
        }
        if let Some(status) = &self.status {
            if note.frontmatter.status.as_deref() != Some(status.as_str()) {
                return false;
            }
        }
        if self.since.is_some() || self.until.is_some() {
            let date = note
                .frontmatter
                .last_verified
                .as_deref()
                .or(note.frontmatter.updated.as_deref())
                .or(note.frontmatter.created.as_deref())
                .or(note.frontmatter.decided_at.as_deref())
                .unwrap_or("")
                .to_string();
            if date.is_empty() {
                return false;
            }
            if let Some(since) = &self.since {
                if date.as_str() < since.as_str() {
                    return false;
                }
            }
            if let Some(until) = &self.until {
                if date.as_str() > until.as_str() {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub rel_path: String,
    pub title: String,
    pub note_type: NoteType,
    pub score: f32,
    pub excerpt: String,
}

impl SearchHit {
    pub fn render(&self) -> String {
        format!(
            "[{}] {}  ({:.1})\n  {}\n  {}\n",
            self.note_type, self.title, self.score, self.rel_path, self.excerpt
        )
    }
}

pub fn tokens(query: &str) -> Vec<String> {
    query
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() >= 2 && !STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// How well `token` matches a single word, 0.0 to 1.0.
fn word_match(token: &str, word: &str) -> f32 {
    if word == token {
        return 1.0;
    }
    if word.starts_with(token) && token.len() >= 3 {
        return 0.8;
    }
    if token.len() >= 4 && word.len() >= 4 && levenshtein(token, word) <= 1 {
        return 0.6;
    }
    0.0
}

fn field_score(token: &str, field: &str) -> f32 {
    let mut best: f32 = 0.0;
    if field.to_ascii_lowercase().contains(token) {
        best = best.max(0.9);
    }
    for word in field
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        best = best.max(word_match(token, word));
        if best >= 1.0 {
            break;
        }
    }
    best
}

fn excerpt_for(note: &Note, tokens: &[String]) -> String {
    let body = note.body.replace('\n', " ");
    let lowered = body.to_ascii_lowercase();
    for token in tokens {
        if let Some(pos) = lowered.find(token.as_str()) {
            let start = lowered[..pos].char_indices().rev().nth(40).map(|(i, _)| i).unwrap_or(0);
            let end = (pos + 120).min(body.len());
            let end = if body.is_char_boundary(end) { end } else { pos };
            return format!("...{}...", body[start..end].trim());
        }
    }
    body.chars().take(140).collect::<String>().trim().to_string()
}

/// Notes to score: the full-text index when it is fresh and can match, the
/// whole vault otherwise. A typo the index cannot match is exactly the case
/// fuzzy scoring exists for, so a miss falls back rather than returning nothing.
fn candidate_notes(vault: &Vault, query_tokens: &[String]) -> Result<Vec<Note>> {
    if let Some(rels) = crate::index::candidates(vault, query_tokens) {
        if !rels.is_empty() {
            let mut notes = Vec::new();
            for rel in rels {
                if let Ok(note) = vault.load(&rel) {
                    notes.push(note);
                }
            }
            if !notes.is_empty() {
                return Ok(notes);
            }
        }
    }
    Ok(vault.scan()?.notes)
}

/// Fuzzy search across titles, bodies, entities and tags.
pub fn search(vault: &Vault, query: &str, filters: &Filters) -> Result<Vec<SearchHit>> {
    let query_tokens = tokens(query);
    let notes = candidate_notes(vault, &query_tokens)?;
    let mut hits = Vec::new();
    for note in notes {
        if !filters.matches(&note) {
            continue;
        }
        let mut score = 0.0f32;
        if !query_tokens.is_empty() {
            let title = note.title();
            let entities = format!(
                "{} {} {}",
                note.frontmatter.people.join(" "),
                note.frontmatter.projects.join(" "),
                note.frontmatter.tags.join(" ")
            );
            let body = note.body.clone();
            for token in &query_tokens {
                score += 5.0 * field_score(token, &title);
                score += 3.0 * field_score(token, &entities);
                score += 1.0 * field_score(token, &body);
            }
            score /= query_tokens.len() as f32;
            if score <= 0.0 {
                continue;
            }
        }
        hits.push(SearchHit {
            excerpt: excerpt_for(&note, &query_tokens),
            rel_path: note.rel_path.clone(),
            title: note.title(),
            note_type: note.note_type(),
            score,
        });
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    Ok(hits)
}

/// Decision cards matching a query. No query means the whole ledger.
pub fn decision_cards(vault: &Vault, query: &str) -> Result<Vec<DecisionCard>> {
    let query_tokens = tokens(query);
    let mut cards = ledger::cards(vault)?;
    if query_tokens.is_empty() {
        return Ok(cards);
    }

    let decisions = ledger::all(vault)?;
    let mut scored: Vec<(f32, DecisionCard)> = Vec::new();
    for card in cards.drain(..) {
        let body = decisions
            .iter()
            .find(|n| n.rel_path == card.rel_path)
            .map(|n| n.body.clone())
            .unwrap_or_default();
        let mut score = 0.0f32;
        for token in &query_tokens {
            score += 5.0 * field_score(token, &card.title);
            score += 3.0 * field_score(token, &card.decision_id);
            score += 1.0 * field_score(token, &body);
        }
        if score > 0.0 {
            scored.push((score, card));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored.into_iter().map(|(_, card)| card).collect())
}

/// "What did we decide about Okta" -> the Okta decision cards, ranked.
pub fn ask(vault: &Vault, question: &str) -> Result<Vec<DecisionCard>> {
    let lowered = question.to_ascii_lowercase();
    let mut subject = lowered.as_str();
    for lead in [
        "what did we decide about",
        "what did we decide on",
        "what have we decided about",
        "what was the decision about",
        "what was the decision on",
        "what is the decision on",
        "did we decide about",
        "decision on",
        "decision about",
        "who decided about",
    ] {
        if let Some(rest) = lowered.strip_prefix(lead) {
            subject = rest;
            break;
        }
    }
    let cleaned = subject.trim().trim_end_matches(['?', '.', '!']).trim();
    let query = if cleaned.is_empty() { question } else { cleaned };
    decision_cards(vault, query)
}

/// Open questions the ledger has not answered yet.
pub fn open_questions(vault: &Vault) -> Result<Vec<DecisionCard>> {
    Ok(ledger::cards(vault)?
        .into_iter()
        .filter(|c| c.status == DecisionStatus::Proposed)
        .collect())
}

/// Every decision reference a note cites.
pub fn citations_in(note: &Note) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for token in note
        .search_text()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
    {
        let upper = token.to_ascii_uppercase();
        if upper.starts_with("DR-") && crate::ids::parse_decision_id(&upper).is_some() {
            out.insert(upper);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{self, NewDecision};
    use crate::vault::test_support::temp_vault;

    fn seed(vault: &Vault) {
        ledger::create(
            vault,
            &NewDecision::new("Use Okta for SSO")
                .accepted()
                .with_owner("alice")
                .with_decision("Okta won on price and admin tooling."),
        )
        .expect("okta");
        ledger::create(
            vault,
            &NewDecision::new("Move billing to Razorpay")
                .accepted()
                .with_decision("Razorpay handles INR mandates."),
        )
        .expect("billing");
        vault
            .write(
                "meetings/2026-03-04.md",
                "---\ntype: meeting\ntitle: SSO kickoff\n---\nWe agreed to use Okta.\n",
            )
            .expect("meeting");
    }

    #[test]
    fn search_finds_notes_by_body_and_title() {
        let vault = temp_vault();
        seed(&vault);
        let hits = search(&vault, "razorpay", &Filters::default()).expect("search");
        assert_eq!(hits[0].title, "Move billing to Razorpay");
        let hits = search(&vault, "kickoff", &Filters::default()).expect("search");
        assert!(hits.iter().any(|h| h.title == "SSO kickoff"));
    }

    #[test]
    fn fuzzy_typos_still_match() {
        let vault = temp_vault();
        seed(&vault);
        let hits = search(&vault, "razorpy", &Filters::default()).expect("search");
        assert!(!hits.is_empty(), "typo should still hit");
    }

    #[test]
    fn filters_narrow_by_type_and_person() {
        let vault = temp_vault();
        seed(&vault);
        let mut filters = Filters::default();
        filters.note_type = Some(NoteType::Decision);
        let hits = search(&vault, "okta", &filters).expect("search");
        assert!(hits.iter().all(|h| h.note_type == NoteType::Decision));

        let mut filters = Filters::default();
        filters.person = Some("alice".into());
        let hits = search(&vault, "", &filters).expect("search");
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn ask_returns_the_decision_card_not_prose() {
        let vault = temp_vault();
        seed(&vault);
        let cards = ask(&vault, "What did we decide about Okta?").expect("ask");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].decision_id, "DR-0001");
        assert_eq!(cards[0].status, DecisionStatus::Accepted);
        assert!(cards[0].render().contains("Use Okta for SSO"));
    }

    #[test]
    fn ask_with_no_match_returns_nothing() {
        let vault = temp_vault();
        seed(&vault);
        let cards = ask(&vault, "What did we decide about Kubernetes?").expect("ask");
        assert!(cards.is_empty());
    }

    #[test]
    fn citations_are_collected_from_a_note() {
        let note = Note::parse("x.md", "---\ntype: meeting\n---\nsee DR-0001 and dr-12.\n").expect("parse");
        let cites = citations_in(&note);
        assert!(cites.contains("DR-0001"));
    }
}
