//! Auto-link suggestions.
//!
//! Suggestions come from two signals: a shared entity (a person or a project
//! named in both notes) and embedding similarity. Nothing is ever accepted
//! automatically — an accepted link is written into the frontmatter `links`,
//! `people` and `projects` arrays, and the body is never touched.

use serde::Serialize;

use crate::embed::{self, Embedder, MIN_SIMILARITY};
use crate::error::{DayDocError, Result};
use crate::note::Note;
use crate::vault::{now_iso, Vault};

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub rel_path: String,
    pub title: String,
    pub score: f32,
    pub reasons: Vec<String>,
}

impl Suggestion {
    pub fn render(&self) -> String {
        format!(
            "{} ({:.2}) — {}\n",
            self.title,
            self.score,
            self.reasons.join("; ")
        )
    }
}

fn shared_entities(note: &Note, other: &Note) -> Vec<String> {
    let mut reasons = Vec::new();
    for person in &note.frontmatter.people {
        if other
            .frontmatter
            .people
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(person))
        {
            reasons.push(format!("shares person {person}"));
        }
    }
    for project in &note.frontmatter.projects {
        if other
            .frontmatter
            .projects
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(project))
        {
            reasons.push(format!("shares project {project}"));
        }
    }
    reasons
}

/// Suggestions for one note. `min_score` is the embedding floor; a shared entity
/// always qualifies, because a name in common is a fact rather than a guess.
pub fn suggest_for<E: Embedder>(
    vault: &Vault,
    embedder: &E,
    note_rel: &str,
    limit: usize,
    min_score: f32,
) -> Result<Vec<Suggestion>> {
    let note = vault.load(note_rel)?;
    let notes = vault.scan()?.notes;
    let target = embedder.embed(&note.search_text());

    let mut suggestions: Vec<Suggestion> = Vec::new();
    for other in &notes {
        if other.rel_path == note.rel_path {
            continue;
        }
        if note.frontmatter.links.contains(&other.rel_path) {
            continue;
        }
        let mut reasons = shared_entities(&note, other);
        let score = embed::cosine(&target, &embedder.embed(&other.search_text()));
        if score < min_score && reasons.is_empty() {
            continue;
        }
        if reasons.is_empty() {
            reasons.push(if embedder.is_semantic() {
                "similar meaning".to_string()
            } else {
                "shared vocabulary".to_string()
            });
        }
        suggestions.push(Suggestion {
            rel_path: other.rel_path.clone(),
            title: other.title(),
            score,
            reasons,
        });
    }

    suggestions.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    suggestions.truncate(limit.max(1));
    Ok(suggestions)
}

/// Suggestions for every note that has not been linked yet, best first.
pub fn suggest_all<E: Embedder>(
    vault: &Vault,
    embedder: &E,
    limit: usize,
    min_score: f32,
) -> Result<Vec<(String, Vec<Suggestion>)>> {
    let mut out: Vec<(String, Vec<Suggestion>)> = Vec::new();
    for note in vault.scan()?.notes {
        if note.note_type() == crate::types::NoteType::Decision {
            continue;
        }
        if !note.frontmatter.links.is_empty() {
            continue;
        }
        let suggestions = suggest_for(vault, embedder, &note.rel_path, limit, min_score)?;
        if !suggestions.is_empty() {
            out.push((note.rel_path, suggestions));
        }
    }
    Ok(out)
}

/// Accept one suggestion: link the two notes and carry the entities across.
/// The body is never edited.
pub fn accept(vault: &Vault, note_rel: &str, target_rel: &str) -> Result<Note> {
    if note_rel == target_rel {
        return Err(DayDocError::Config(
            "a note cannot link to itself".to_string(),
        ));
    }
    let mut note = vault.load(note_rel)?;
    let target = vault.load(target_rel)?;

    if !note.frontmatter.links.contains(&target.rel_path) {
        note.frontmatter.links.push(target.rel_path.clone());
    }
    for person in &target.frontmatter.people {
        if !note
            .frontmatter
            .people
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(person))
        {
            note.frontmatter.people.push(person.clone());
        }
    }
    for project in &target.frontmatter.projects {
        if !note
            .frontmatter
            .projects
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(project))
        {
            note.frontmatter.projects.push(project.clone());
        }
    }
    note.frontmatter.updated = Some(now_iso());
    vault.save(&note)?;
    vault.audit(
        "link-accepted",
        note_rel,
        &format!("linked to {target_rel}"),
    )?;
    Ok(note)
}

pub fn default_min_score() -> f32 {
    MIN_SIMILARITY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::HashEmbedder;
    use crate::vault::test_support::temp_vault;

    fn two_notes() -> Vault {
        let vault = temp_vault();
        vault
            .write(
                "meetings/okta-1.md",
                "---\nid: 01A\ntype: meeting\ntitle: Okta SSO pilot\npeople: [alice]\n---\nokta sso pilot admin tooling enterprise pricing\n",
            )
            .expect("a");
        vault
            .write(
                "meetings/okta-2.md",
                "---\nid: 01B\ntype: meeting\ntitle: Okta SSO review\npeople: [alice]\n---\nokta sso pilot admin tooling enterprise pricing review\n",
            )
            .expect("b");
        vault
            .write(
                "meetings/payroll.md",
                "---\nid: 01C\ntype: meeting\ntitle: Payroll filing\n---\nstatutory filing interest penalty challan\n",
            )
            .expect("c");
        vault
    }

    #[test]
    fn a_shared_person_is_a_reason_even_below_the_floor() {
        let vault = two_notes();
        let embedder = HashEmbedder::default();
        let suggestions =
            suggest_for(&vault, &embedder, "meetings/okta-1.md", 5, 0.99).expect("suggest");
        let review = suggestions
            .iter()
            .find(|s| s.rel_path == "meetings/okta-2.md")
            .expect("the shared-person note is suggested");
        assert!(review.reasons.iter().any(|r| r.contains("shares person alice")));
    }

    #[test]
    fn vocabulary_similarity_ranks_the_related_note_first() {
        let vault = two_notes();
        let embedder = HashEmbedder::default();
        let suggestions =
            suggest_for(&vault, &embedder, "meetings/okta-1.md", 5, 0.2).expect("suggest");
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].rel_path, "meetings/okta-2.md");
        assert!(suggestions[0].score > 0.2);
    }

    #[test]
    fn an_unrelated_note_is_not_suggested() {
        let vault = two_notes();
        let embedder = HashEmbedder::default();
        let suggestions =
            suggest_for(&vault, &embedder, "meetings/payroll.md", 5, 0.5).expect("suggest");
        assert!(suggestions
            .iter()
            .all(|s| s.rel_path != "meetings/okta-1.md"));
    }

    #[test]
    fn nothing_is_linked_until_it_is_accepted() {
        let vault = two_notes();
        let embedder = HashEmbedder::default();
        let _ = suggest_for(&vault, &embedder, "meetings/okta-1.md", 5, 0.2).expect("suggest");
        let note = vault.load("meetings/okta-1.md").expect("load");
        assert!(note.frontmatter.links.is_empty(), "a suggestion wrote itself");
    }

    #[test]
    fn accepting_links_and_copies_entities_without_touching_the_body() {
        let vault = two_notes();
        let before = vault.load("meetings/okta-1.md").expect("load").body;
        let note = accept(&vault, "meetings/okta-1.md", "meetings/okta-2.md").expect("accept");
        assert_eq!(note.frontmatter.links, vec!["meetings/okta-2.md".to_string()]);
        assert_eq!(note.body, before, "the body must not change");

        // A second accept is a no-op, not a duplicate.
        let again = accept(&vault, "meetings/okta-1.md", "meetings/okta-2.md").expect("accept");
        assert_eq!(again.frontmatter.links.len(), 1);

        let audit = vault.read("_meta/audit.jsonl").expect("audit");
        assert!(audit.contains("link-accepted"));
    }

    #[test]
    fn entities_come_across_with_the_link() {
        let vault = temp_vault();
        vault
            .write(
                "inbox/a.md",
                "---\nid: 01A\ntype: idea\ntitle: A\n---\nbody\n",
            )
            .expect("a");
        vault
            .write(
                "inbox/b.md",
                "---\nid: 01B\ntype: idea\ntitle: B\npeople: [priya]\nprojects: [Atlas]\n---\nbody\n",
            )
            .expect("b");
        let note = accept(&vault, "inbox/a.md", "inbox/b.md").expect("accept");
        assert_eq!(note.frontmatter.people, vec!["priya".to_string()]);
        assert_eq!(note.frontmatter.projects, vec!["Atlas".to_string()]);
    }

    #[test]
    fn a_note_cannot_link_to_itself() {
        let vault = two_notes();
        assert!(accept(&vault, "meetings/okta-1.md", "meetings/okta-1.md").is_err());
    }
}
