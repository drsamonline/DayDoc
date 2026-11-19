//! A note: frontmatter plus body, plus the section helpers the decision
//! templates rely on.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::frontmatter::{self, Frontmatter};
use crate::types::NoteType;

#[derive(Debug, Clone)]
pub struct Note {
    /// Path relative to the vault root, always with forward slashes.
    pub rel_path: String,
    pub frontmatter: Frontmatter,
    pub body: String,
}

impl Note {
    pub fn parse(rel_path: impl Into<String>, raw: &str) -> Result<Note> {
        let (yaml, body) = frontmatter::split(raw)?;
        let fm = match yaml {
            Some(block) => Frontmatter::parse(&block)?,
            None => Frontmatter::default(),
        };
        Ok(Note {
            rel_path: rel_path.into(),
            frontmatter: fm,
            body,
        })
    }

    pub fn render(&self) -> String {
        let mut out = String::from("---\n");
        out.push_str(&self.frontmatter.render());
        out.push_str("---\n");
        let body = self.body.trim_start_matches('\n');
        out.push_str(body);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    pub fn note_type(&self) -> NoteType {
        self.frontmatter.note_type().unwrap_or(NoteType::Inbox)
    }

    pub fn title(&self) -> String {
        if !self.frontmatter.title.trim().is_empty() {
            return self.frontmatter.title.clone();
        }
        for line in self.body.lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("# ") {
                return rest.trim().to_string();
            }
        }
        let stem = self
            .rel_path
            .rsplit('/')
            .next()
            .unwrap_or(&self.rel_path)
            .trim_end_matches(".md");
        stem.to_string()
    }

    /// `## Section` blocks, keyed by lowercased heading.
    pub fn sections(&self) -> BTreeMap<String, String> {
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        let mut current: Option<String> = None;
        let mut buffer = String::new();
        for line in self.body.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                if let Some(name) = current.take() {
                    map.insert(name, buffer.trim().to_string());
                }
                buffer.clear();
                current = Some(heading.trim().to_ascii_lowercase());
            } else if current.is_some() {
                buffer.push_str(line);
                buffer.push('\n');
            }
        }
        if let Some(name) = current {
            map.insert(name, buffer.trim().to_string());
        }
        map
    }

    pub fn section(&self, name: &str) -> String {
        self.sections()
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    /// Everything searchable: title, body, entities, tags.
    pub fn search_text(&self) -> String {
        let fm = &self.frontmatter;
        let mut parts: Vec<String> = vec![self.title(), self.body.clone()];
        parts.extend(fm.people.iter().cloned());
        parts.extend(fm.projects.iter().cloned());
        parts.extend(fm.tags.iter().cloned());
        if let Some(id) = &fm.decision_id {
            parts.push(id.clone());
        }
        parts.join("\n")
    }
}

/// Render the canonical decision body. Order is fixed so the diff of an
/// amended decision shows only what changed.
pub fn decision_body(context: &str, alternatives: &str, decision: &str, consequences: &str) -> String {
    let mut out = String::new();
    let blocks = [
        ("Context", context),
        ("Alternatives", alternatives),
        ("Decision", decision),
        ("Consequences", consequences),
    ];
    for (heading, text) in blocks {
        out.push_str("## ");
        out.push_str(heading);
        out.push_str("\n\n");
        let body = text.trim();
        if body.is_empty() {
            out.push_str("_Not recorded._\n\n");
        } else {
            out.push_str(body);
            out.push_str("\n\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_render_round_trip() {
        let raw = "---\nid: 01ABC\ntype: decision\ntitle: Use Okta\nstatus: accepted\n---\n\n## Context\n\nWe needed SSO.\n";
        let note = Note::parse("decisions/DR-0007.md", raw).expect("parse");
        assert_eq!(note.title(), "Use Okta");
        assert_eq!(note.note_type(), NoteType::Decision);
        assert_eq!(note.section("context"), "We needed SSO.");
        let again = Note::parse("decisions/DR-0007.md", &note.render()).expect("reparse");
        assert_eq!(again.frontmatter.title, note.frontmatter.title);
        assert_eq!(again.body.trim(), note.body.trim());
    }

    #[test]
    fn sections_are_keyed_case_insensitively() {
        let note = Note::parse(
            "x.md",
            "---\ntype: decision\n---\n## Context\na\n## Decision\nb\n",
        )
        .expect("parse");
        let sections = note.sections();
        assert_eq!(sections.get("context").map(String::as_str), Some("a"));
        assert_eq!(sections.get("decision").map(String::as_str), Some("b"));
    }

    #[test]
    fn title_falls_back_to_heading_then_stem() {
        let note = Note::parse("ideas/thing.md", "# A heading\n").expect("parse");
        assert_eq!(note.title(), "A heading");
        let note = Note::parse("ideas/thing.md", "plain\n").expect("parse");
        assert_eq!(note.title(), "thing");
    }

    #[test]
    fn decision_body_has_four_sections() {
        let body = decision_body("c", "a", "d", "x");
        for heading in ["## Context", "## Alternatives", "## Decision", "## Consequences"] {
            assert!(body.contains(heading));
        }
    }
}
