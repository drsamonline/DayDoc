//! The decision ledger.
//!
//! Decisions are objects with two identities: a ULID for linking and a
//! `DR-NNNN` reference for humans and for supersession pointers. History is
//! appended, never rewritten: reversing an accepted decision means writing a
//! new one that supersedes it.

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::Serialize;

use crate::error::{DayDocError, Result};
use crate::ids;
use crate::note::{decision_body, Note};
use crate::types::{DecisionStatus, NoteType};
use crate::vault::{now_iso, today, Vault};

/// Input for a new decision. Empty sections are written as `_Not recorded._`
/// rather than omitted, so the template stays recognisable.
#[derive(Debug, Clone)]
pub struct NewDecision {
    pub title: String,
    pub context: String,
    pub alternatives: String,
    pub decision: String,
    pub consequences: String,
    pub owner: Option<String>,
    pub source: Option<String>,
    pub supersedes: Option<String>,
    pub status: DecisionStatus,
    pub decided_at: Option<String>,
    pub people: Vec<String>,
    pub projects: Vec<String>,
    pub tags: Vec<String>,
}

impl Default for NewDecision {
    fn default() -> Self {
        NewDecision {
            title: String::new(),
            context: String::new(),
            alternatives: String::new(),
            decision: String::new(),
            consequences: String::new(),
            owner: None,
            source: None,
            supersedes: None,
            status: DecisionStatus::Proposed,
            decided_at: None,
            people: Vec::new(),
            projects: Vec::new(),
            tags: Vec::new(),
        }
    }
}

impl NewDecision {
    pub fn new(title: impl Into<String>) -> Self {
        NewDecision {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn accepted(mut self) -> Self {
        self.status = DecisionStatus::Accepted;
        self
    }

    pub fn with_decision(mut self, text: impl Into<String>) -> Self {
        self.decision = text.into();
        self
    }

    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = Some(owner.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn superseding(mut self, old: impl Into<String>) -> Self {
        self.supersedes = Some(old.into());
        self
    }
}

/// A one-line result card: what was decided, when, by whom, and what it
/// replaced. Search returns these instead of prose.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionCard {
    pub decision_id: String,
    pub title: String,
    pub status: DecisionStatus,
    pub decided_at: Option<String>,
    pub owner: Option<String>,
    pub source: Option<String>,
    pub supersedes: Option<String>,
    pub superseded_by: Option<String>,
    pub cited_by: usize,
    pub rel_path: String,
}

impl DecisionCard {
    /// Human-readable card, one fact per line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}  {}\n", self.decision_id, self.title));
        out.push_str(&format!(
            "  {} · {} · {}\n",
            self.status,
            self.decided_at.as_deref().unwrap_or("undated"),
            self.owner.as_deref().unwrap_or("no owner")
        ));
        if let Some(source) = &self.source {
            out.push_str(&format!("  source: {source}\n"));
        }
        out.push_str(&format!(
            "  supersedes {} · superseded by {}\n",
            self.supersedes.as_deref().unwrap_or("none"),
            self.superseded_by.as_deref().unwrap_or("none")
        ));
        out.push_str(&format!("  cited by {} note(s)\n", self.cited_by));
        out
    }

    pub fn one_line(&self) -> String {
        format!("{} — {} ({})", self.decision_id, self.title, self.status)
    }
}

/// Allocate the next `DR-NNNN` from the SQLite counter, in one immediate
/// transaction so two writers can never take the same number.
pub fn allocate(vault: &Vault) -> Result<String> {
    let mut conn = vault.db()?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current: Option<i64> = tx
        .query_row(
            "SELECT value FROM counters WHERE name = 'decision_id'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let next = current.unwrap_or(0) + 1;
    tx.execute(
        "INSERT INTO counters (name, value) VALUES ('decision_id', ?1)
         ON CONFLICT(name) DO UPDATE SET value = excluded.value",
        params![next],
    )?;
    tx.commit()?;
    Ok(ids::decision_id(next as u64))
}

pub fn path_for(decision_id: &str) -> String {
    format!("decisions/{decision_id}.md")
}

/// Canonicalise any accepted spelling of a decision reference to `DR-NNNN`.
pub fn canonical_id(raw: &str) -> Result<String> {
    ids::parse_decision_id(raw)
        .map(ids::decision_id)
        .ok_or_else(|| DayDocError::DecisionNotFound(raw.to_string()))
}

/// Create a decision. If `supersedes` is set, the older decision is marked
/// superseded in the same operation — both stay readable forever.
pub fn create(vault: &Vault, new: &NewDecision) -> Result<Note> {
    let title = new.title.trim();
    if title.is_empty() {
        return Err(DayDocError::Ledger(
            "a decision needs a title; a blank record is not a decision".to_string(),
        ));
    }

    let supersedes = match &new.supersedes {
        Some(raw) => {
            let canonical = canonical_id(raw)?;
            // The old decision must exist before we point at it.
            let _ = find(vault, &canonical)?;
            Some(canonical)
        }
        None => None,
    };

    let decision_id = allocate(vault)?;
    let now = now_iso();
    let date = new.decided_at.clone().unwrap_or_else(today);
    let mut note = Note::parse(
        path_for(&decision_id),
        "---\ntype: decision\n---\n",
    )?;
    note.frontmatter.id = ids::new_ulid();
    note.frontmatter.note_type = NoteType::Decision.as_str().to_string();
    note.frontmatter.decision_id = Some(decision_id.clone());
    note.frontmatter.title = title.to_string();
    note.frontmatter.status = Some(new.status.as_str().to_string());
    note.frontmatter.owner = new.owner.clone();
    // A proposed decision has not been decided yet; only an accepted one is dated.
    note.frontmatter.decided_at = if new.status == DecisionStatus::Accepted {
        Some(date.clone())
    } else {
        new.decided_at.clone()
    };
    note.frontmatter.supersedes = supersedes.clone();
    note.frontmatter.source = new.source.clone();
    note.frontmatter.last_verified = Some(date.clone());
    note.frontmatter.created = Some(now.clone());
    note.frontmatter.updated = Some(now);
    note.frontmatter.people = new.people.clone();
    note.frontmatter.projects = new.projects.clone();
    note.frontmatter.tags = new.tags.clone();
    note.body = decision_body(
        &new.context,
        &new.alternatives,
        &new.decision,
        &new.consequences,
    );

    vault.create(&note.rel_path, &note.render())?;
    vault.audit(
        "decision-created",
        &decision_id,
        &format!("status={} title={title}", new.status),
    )?;

    if let Some(old_id) = &supersedes {
        mark_superseded(vault, old_id, &decision_id)?;
    }
    Ok(note)
}

/// Mark `old_id` as superseded by `new_id`, writing both pointer and date.
fn mark_superseded(vault: &Vault, old_id: &str, new_id: &str) -> Result<()> {
    let mut old = find(vault, old_id)?;
    old.frontmatter.status = Some(DecisionStatus::Superseded.as_str().to_string());
    old.frontmatter.superseded_by = Some(new_id.to_string());
    old.frontmatter.superseded_at = Some(today());
    old.frontmatter.updated = Some(now_iso());
    vault.save(&old)?;
    vault.audit(
        "decision-superseded",
        old_id,
        &format!("superseded_by={new_id}"),
    )?;
    Ok(())
}

/// Create a decision that replaces `old`, without hand-writing the pointer.
pub fn supersede(vault: &Vault, old: &str, new: &NewDecision) -> Result<Note> {
    let canonical = canonical_id(old)?;
    let mut request = new.clone();
    request.supersedes = Some(canonical);
    if request.status == DecisionStatus::Proposed {
        request.status = DecisionStatus::Accepted;
    }
    create(vault, &request)
}

/// Undo a supersession that should not have happened. Restores both decisions
/// to `accepted`, clears the link on both sides, and audits the correction.
/// The new decision stays in the ledger — it is not deleted.
pub fn revert_supersession(vault: &Vault, new_ref: &str, reason: &str) -> Result<(Note, Note)> {
    let new_id = canonical_id(new_ref)?;
    let mut new = find(vault, &new_id)?;
    let old_ref = new.frontmatter.supersedes.clone().ok_or_else(|| {
        DayDocError::InvalidTransition(format!(
            "{new_id} does not supersede anything, so there is nothing to revert"
        ))
    })?;
    let old_id = canonical_id(&old_ref)?;
    let mut old = find(vault, &old_id)?;

    new.frontmatter.supersedes = None;
    new.frontmatter.updated = Some(now_iso());
    old.frontmatter.superseded_by = None;
    old.frontmatter.superseded_at = None;
    old.frontmatter.status = Some(DecisionStatus::Accepted.as_str().to_string());
    old.frontmatter.updated = Some(now_iso());

    vault.save(&old)?;
    vault.save(&new)?;
    vault.audit(
        "supersession-reverted",
        &old_id,
        &format!("unlinked from {new_id}; reason: {reason}"),
    )?;
    Ok((new, old))
}

/// Move a decision between lifecycle states, refusing impossible moves.
pub fn transition(vault: &Vault, reference: &str, to: DecisionStatus) -> Result<Note> {
    let canonical = canonical_id(reference)?;
    let mut note = find(vault, &canonical)?;
    let current = note
        .frontmatter
        .status
        .as_deref()
        .and_then(DecisionStatus::parse)
        .unwrap_or(DecisionStatus::Proposed);
    if current == to {
        return Ok(note);
    }
    if !current.can_transition_to(to) {
        return Err(DayDocError::InvalidTransition(format!(
            "{canonical}: {current} cannot become {to}; supersede it with a new decision instead"
        )));
    }
    note.frontmatter.status = Some(to.as_str().to_string());
    note.frontmatter.updated = Some(now_iso());
    if to == DecisionStatus::Accepted {
        note.frontmatter.decided_at.get_or_insert_with(today);
    }
    vault.save(&note)?;
    vault.audit("decision-transition", &canonical, &format!("{current} -> {to}"))?;
    Ok(note)
}

pub fn accept(vault: &Vault, reference: &str) -> Result<Note> {
    transition(vault, reference, DecisionStatus::Accepted)
}

pub fn reject(vault: &Vault, reference: &str) -> Result<Note> {
    transition(vault, reference, DecisionStatus::Rejected)
}

/// Mark a decision as re-verified today.
pub fn verify(vault: &Vault, reference: &str) -> Result<Note> {
    let canonical = canonical_id(reference)?;
    let mut note = find(vault, &canonical)?;
    note.frontmatter.last_verified = Some(today());
    note.frontmatter.updated = Some(now_iso());
    vault.save(&note)?;
    vault.audit("decision-verified", &canonical, "last_verified updated")?;
    Ok(note)
}

/// Look up a decision by `DR-NNNN`, by ULID, or by title.
pub fn find(vault: &Vault, reference: &str) -> Result<Note> {
    if let Some(number) = ids::parse_decision_id(reference) {
        let canonical = ids::decision_id(number);
        let rel = path_for(&canonical);
        if vault.exists(&rel) {
            return vault.load(&rel);
        }
        return Err(DayDocError::DecisionNotFound(canonical));
    }

    let needle = reference.trim().to_ascii_lowercase();
    for note in all(vault)? {
        if note.frontmatter.id == reference {
            return Ok(note);
        }
        if note.title().to_ascii_lowercase() == needle {
            return Ok(note);
        }
    }
    Err(DayDocError::DecisionNotFound(reference.to_string()))
}

/// Every decision in the ledger, ordered by number.
pub fn all(vault: &Vault) -> Result<Vec<Note>> {
    let mut decisions: Vec<Note> = vault
        .scan()?
        .notes
        .into_iter()
        .filter(|n| n.note_type() == NoteType::Decision)
        .collect();
    decisions.sort_by_key(|n| {
        n.frontmatter
            .decision_id
            .as_deref()
            .and_then(ids::parse_decision_id)
            .unwrap_or(u64::MAX)
    });
    Ok(decisions)
}

pub fn status_of(note: &Note) -> DecisionStatus {
    note.frontmatter
        .status
        .as_deref()
        .and_then(DecisionStatus::parse)
        .unwrap_or(DecisionStatus::Proposed)
}

/// Count the notes that cite each decision, in one pass over the vault.
pub fn citation_counts(vault: &Vault) -> Result<std::collections::BTreeMap<String, usize>> {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for note in vault.scan()?.notes {
        if note.note_type() == NoteType::Decision {
            continue;
        }
        let haystack = note.search_text();
        let mut seen: Vec<String> = Vec::new();
        for token in haystack.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
            if let Some(_n) = ids::parse_decision_id(token) {
                if token.to_ascii_uppercase().starts_with("DR-") {
                    let canonical = token.to_ascii_uppercase();
                    if !seen.contains(&canonical) {
                        seen.push(canonical);
                    }
                }
            }
        }
        for id in seen {
            *counts.entry(id).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

/// Build a card for a decision note.
pub fn card(note: &Note, citations: &std::collections::BTreeMap<String, usize>) -> DecisionCard {
    let decision_id = note.frontmatter.decision_id.clone().unwrap_or_default();
    DecisionCard {
        cited_by: citations.get(&decision_id).copied().unwrap_or(0),
        decision_id,
        title: note.title(),
        status: status_of(note),
        decided_at: note.frontmatter.decided_at.clone(),
        owner: note.frontmatter.owner.clone(),
        source: note.frontmatter.source.clone(),
        supersedes: note.frontmatter.supersedes.clone(),
        superseded_by: note.frontmatter.superseded_by.clone(),
        rel_path: note.rel_path.clone(),
    }
}

/// Cards for the whole ledger.
pub fn cards(vault: &Vault) -> Result<Vec<DecisionCard>> {
    let citations = citation_counts(vault)?;
    Ok(all(vault)?
        .iter()
        .map(|n| card(n, &citations))
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    fn seed(vault: &Vault, title: &str) -> Note {
        create(
            vault,
            &NewDecision::new(title).accepted().with_owner("alice"),
        )
        .expect("create decision")
    }

    #[test]
    fn ids_are_allocated_in_order_and_never_reused() {
        let vault = temp_vault();
        let a = seed(&vault, "Adopt Okta for SSO");
        let b = seed(&vault, "Move billing to Razorpay");
        assert_eq!(a.frontmatter.decision_id.as_deref(), Some("DR-0001"));
        assert_eq!(b.frontmatter.decision_id.as_deref(), Some("DR-0002"));
        assert!(vault.exists("decisions/DR-0001.md"));

        // Deleting a note must not free its number.
        vault.trash("decisions/DR-0002.md").expect("trash");
        let c = seed(&vault, "Third decision");
        assert_eq!(c.frontmatter.decision_id.as_deref(), Some("DR-0003"));
    }

    #[test]
    fn supersession_sets_both_sides_and_keeps_history() {
        let vault = temp_vault();
        let old = seed(&vault, "Use Okta for SSO");
        let new = supersede(
            &vault,
            "DR-0001",
            &NewDecision::new("Use Entra ID for SSO").with_decision("Entra wins on price."),
        )
        .expect("supersede");

        assert_eq!(new.frontmatter.supersedes.as_deref(), Some("DR-0001"));
        let reloaded = find(&vault, "DR-0001").expect("old still readable");
        assert_eq!(status_of(&reloaded), DecisionStatus::Superseded);
        assert_eq!(
            reloaded.frontmatter.superseded_by.as_deref(),
            Some("DR-0002")
        );
        assert!(reloaded.frontmatter.superseded_at.is_some());
        assert_eq!(status_of(&new), DecisionStatus::Accepted);

        // History is intact: the old body is untouched.
        assert_eq!(old.body, reloaded.body);
    }

    #[test]
    fn supersession_is_revertible_and_audited() {
        let vault = temp_vault();
        seed(&vault, "Use Okta for SSO");
        supersede(&vault, "DR-0001", &NewDecision::new("Use Entra ID for SSO")).expect("supersede");

        let (new, old) = revert_supersession(&vault, "DR-0002", "wrong call, Okta stays")
            .expect("revert");
        assert!(new.frontmatter.supersedes.is_none());
        assert!(old.frontmatter.superseded_by.is_none());
        assert_eq!(status_of(&old), DecisionStatus::Accepted);
        assert_eq!(status_of(&new), DecisionStatus::Accepted);

        let audit = vault.read("_meta/audit.jsonl").expect("audit log");
        assert!(audit.contains("supersession-reverted"));
    }

    #[test]
    fn revert_without_a_supersession_is_refused() {
        let vault = temp_vault();
        seed(&vault, "Standalone");
        assert!(revert_supersession(&vault, "DR-0001", "why not").is_err());
    }

    #[test]
    fn lifecycle_transitions_are_guarded() {
        let vault = temp_vault();
        create(&vault, &NewDecision::new("Proposed thing")).expect("create");
        accept(&vault, "DR-0001").expect("accept");
        assert!(reject(&vault, "DR-0001").is_err());
        assert!(find(&vault, "DR-0001").is_ok());
        assert!(find(&vault, "DR-0404").is_err());
    }

    #[test]
    fn superseding_a_missing_decision_fails_without_allocating() {
        let vault = temp_vault();
        let err = create(
            &vault,
            &NewDecision::new("Dangling").superseding("DR-9999"),
        );
        assert!(err.is_err());
        // No number burned.
        let a = seed(&vault, "First real one");
        assert_eq!(a.frontmatter.decision_id.as_deref(), Some("DR-0001"));
    }

    #[test]
    fn cards_count_citations() {
        let vault = temp_vault();
        seed(&vault, "Use Okta for SSO");
        vault
            .write(
                "meetings/2026-03-04.md",
                "---\ntype: meeting\ntitle: Kickoff\n---\nWe confirmed DR-0001 and moved on.\n",
            )
            .expect("write meeting");
        let cards = cards(&vault).expect("cards");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].cited_by, 1);
        assert!(cards[0].render().contains("DR-0001"));
    }

    #[test]
    fn find_accepts_title_and_ulid() {
        let vault = temp_vault();
        let note = seed(&vault, "Adopt Okta for SSO");
        assert_eq!(
            find(&vault, "adopt okta for sso").expect("by title").rel_path,
            note.rel_path
        );
        let by_ulid = find(&vault, &note.frontmatter.id).expect("by ulid");
        assert_eq!(by_ulid.rel_path, note.rel_path);
    }
}
