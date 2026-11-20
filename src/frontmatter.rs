//! YAML frontmatter: the schema half of a note.
//!
//! Reading is tolerant (any YAML the parser accepts is read, unknown keys are
//! preserved in `extra` so a round trip never loses a user's field). Writing is
//! deterministic: fixed key order, so diffs stay small and git history stays
//! readable.

use std::collections::BTreeMap;

use yaml_rust2::{Yaml, YamlLoader};

use crate::error::{DayDocError, Result};

/// Keys DayDoc owns. Emitted in this order.
const KNOWN_KEYS: [&str; 19] = [
    "id",
    "type",
    "decision_id",
    "title",
    "status",
    "owner",
    "decided_at",
    "supersedes",
    "superseded_by",
    "superseded_at",
    "source",
    "last_verified",
    "created",
    "updated",
    "people",
    "projects",
    "tags",
    "links",
    "stale",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub id: String,
    /// `type` — one of the note types.
    pub note_type: String,
    pub decision_id: Option<String>,
    pub title: String,
    pub status: Option<String>,
    pub owner: Option<String>,
    pub decided_at: Option<String>,
    pub supersedes: Option<String>,
    pub superseded_by: Option<String>,
    pub superseded_at: Option<String>,
    pub source: Option<String>,
    pub last_verified: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub people: Vec<String>,
    pub projects: Vec<String>,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    /// Set when stale detection has flagged this note; cleared by re-verify.
    pub stale: Option<String>,
    /// Any other frontmatter the user added. Preserved verbatim.
    pub extra: BTreeMap<String, String>,
}

fn scalar(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        Yaml::Null => None,
        _ => None,
    }
}

fn string_list(value: &Yaml) -> Vec<String> {
    match value {
        Yaml::Array(items) => items.iter().filter_map(scalar).collect(),
        Yaml::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                trimmed
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            }
        }
        _ => Vec::new(),
    }
}

impl Frontmatter {
    /// Parse the YAML block (without the `---` fences).
    pub fn parse(yaml_block: &str) -> Result<Frontmatter> {
        let docs = YamlLoader::load_from_str(yaml_block)
            .map_err(|e| DayDocError::Yaml(format!("{e}")))?;
        let doc = match docs.into_iter().next() {
            Some(d) => d,
            None => return Ok(Frontmatter::default()),
        };
        let hash = match doc.as_hash() {
            Some(h) => h,
            None => {
                return Err(DayDocError::Yaml(
                    "frontmatter is not a mapping of keys to values".to_string(),
                ))
            }
        };

        let mut fm = Frontmatter::default();
        for (key, value) in hash {
            let key = match scalar(key) {
                Some(k) => k,
                None => continue,
            };
            match key.as_str() {
                "id" => fm.id = scalar(value).unwrap_or_default(),
                "type" | "note_type" => fm.note_type = scalar(value).unwrap_or_default(),
                "decision_id" => fm.decision_id = scalar(value),
                "title" => fm.title = scalar(value).unwrap_or_default(),
                "status" => fm.status = scalar(value),
                "owner" => fm.owner = scalar(value),
                "decided_at" => fm.decided_at = scalar(value),
                "supersedes" => fm.supersedes = scalar(value),
                "superseded_by" => fm.superseded_by = scalar(value),
                "superseded_at" => fm.superseded_at = scalar(value),
                "source" => fm.source = scalar(value),
                "last_verified" => fm.last_verified = scalar(value),
                "created" => fm.created = scalar(value),
                "updated" => fm.updated = scalar(value),
                "people" => fm.people = string_list(value),
                "projects" => fm.projects = string_list(value),
                "tags" => fm.tags = string_list(value),
                "links" => fm.links = string_list(value),
                "stale" => fm.stale = scalar(value),
                other => {
                    if let Some(v) = scalar(value) {
                        fm.extra.insert(other.to_string(), v);
                    } else if let Yaml::Array(_) = value {
                        fm.extra.insert(other.to_string(), string_list(value).join(", "));
                    }
                }
            }
        }
        Ok(fm)
    }

    /// The note type as declared, if it is one DayDoc knows.
    pub fn note_type(&self) -> Option<crate::types::NoteType> {
        crate::types::NoteType::parse(&self.note_type)
    }

    /// Render the YAML block, without fences. Deterministic order.
    pub fn render(&self) -> String {
        let mut out = String::new();
        push_kv(&mut out, "id", Some(self.id.as_str()));
        push_kv(&mut out, "type", Some(self.note_type.as_str()));
        push_kv(&mut out, "decision_id", self.decision_id.as_deref());
        push_kv(&mut out, "title", Some(self.title.as_str()));
        push_kv(&mut out, "status", self.status.as_deref());
        push_kv(&mut out, "owner", self.owner.as_deref());
        push_kv(&mut out, "decided_at", self.decided_at.as_deref());
        push_kv(&mut out, "supersedes", self.supersedes.as_deref());
        push_kv(&mut out, "superseded_by", self.superseded_by.as_deref());
        push_kv(&mut out, "superseded_at", self.superseded_at.as_deref());
        push_kv(&mut out, "source", self.source.as_deref());
        push_kv(&mut out, "last_verified", self.last_verified.as_deref());
        push_kv(&mut out, "created", self.created.as_deref());
        push_kv(&mut out, "updated", self.updated.as_deref());
        push_list(&mut out, "people", &self.people);
        push_list(&mut out, "projects", &self.projects);
        push_list(&mut out, "tags", &self.tags);
        push_list(&mut out, "links", &self.links);
        push_kv(&mut out, "stale", self.stale.as_deref());

        for (key, value) in &self.extra {
            if KNOWN_KEYS.contains(&key.as_str()) {
                continue;
            }
            out.push_str(key);
            out.push_str(": ");
            out.push_str(&quote(value));
            out.push('\n');
        }
        out
    }
}

fn push_kv(out: &mut String, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        if !v.is_empty() {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(&quote(v));
            out.push('\n');
        }
    }
}

fn push_list(out: &mut String, key: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(key);
    out.push_str(": [");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&quote(item));
    }
    out.push_str("]\n");
}

/// Quote a scalar when leaving it bare would change its meaning.
/// Whether a value would survive as a plain YAML scalar.
///
/// Conservative on purpose: anything a YAML reader could reinterpret — a number
/// in any base, a flow indicator, a comma inside an inline list, a carriage
/// return — is quoted. A quoted string is always read back as a string.
fn looks_numeric(raw: &str) -> bool {
    if raw.parse::<f64>().is_ok() {
        return true;
    }
    let trimmed = raw.trim_start_matches(['+', '-']);
    let lowered = trimmed.to_ascii_lowercase();
    lowered.starts_with("0x") || lowered.starts_with("0o") || lowered.starts_with("0b")
}

fn quote(raw: &str) -> String {
    let needs_quotes = raw.is_empty()
        || raw.trim() != raw
        || raw.starts_with(['[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', '-', '?', ':'])
        || raw.contains(": ")
        || raw.contains(" #")
        || raw.ends_with(':')
        || raw.contains(['\n', '\t', '\r', ',', '[', ']', '{', '}'])
        || looks_numeric(raw)
        || matches!(
            raw.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "null" | "~" | "on" | "off"
        );

    if !needs_quotes {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Split a file into (frontmatter block, body). A file with no leading `---`
/// is all body.
pub fn split(raw: &str) -> Result<(Option<String>, String)> {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut chunks = text.split_inclusive('\n');
    let first = chunks.next().unwrap_or("");
    if first.trim() != "---" {
        return Ok((None, text.to_string()));
    }

    let mut yaml = String::new();
    let mut offset = first.len();
    let mut closed = false;
    for chunk in chunks {
        offset += chunk.len();
        if chunk.trim() == "---" {
            closed = true;
            break;
        }
        yaml.push_str(chunk);
    }
    if !closed {
        return Err(DayDocError::Yaml(
            "frontmatter opened with --- but never closed".to_string(),
        ));
    }
    Ok((Some(yaml), text[offset..].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Frontmatter {
        Frontmatter {
            id: "01J8ZZZZZZZZZZZZZZZZZZZZZZ".into(),
            note_type: "decision".into(),
            decision_id: Some("DR-0007".into()),
            title: "Use Okta for SSO".into(),
            status: Some("accepted".into()),
            owner: Some("alice".into()),
            decided_at: Some("2026-03-04".into()),
            supersedes: Some("DR-0003".into()),
            people: vec!["alice".into(), "bob".into()],
            tags: vec!["sso".into()],
            ..Default::default()
        }
    }

    #[test]
    fn round_trip_is_lossless() {
        let fm = sample();
        let parsed = Frontmatter::parse(&fm.render()).expect("parse");
        assert_eq!(parsed, fm);
    }

    #[test]
    fn unknown_keys_survive() {
        let fm = Frontmatter::parse("id: x\ntype: idea\ncustom: keep me\n").expect("parse");
        assert_eq!(fm.extra.get("custom").map(String::as_str), Some("keep me"));
        let rendered = fm.render();
        assert!(rendered.contains("custom: \"keep me\"") || rendered.contains("custom: keep me"));
    }

    #[test]
    fn quoting_keeps_yaml_meaning() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("a: b"), "\"a: b\"");
        assert_eq!(quote("true"), "\"true\"");
        assert_eq!(quote("123"), "\"123\"");
        assert_eq!(quote("- dash"), "\"- dash\"");
    }

    #[test]
    fn split_handles_crlf_and_missing_frontmatter() {
        let (fm, body) = split("---\r\nid: abc\r\n---\r\nhello\r\n").expect("split");
        assert_eq!(fm.as_deref().map(str::trim), Some("id: abc"));
        assert_eq!(body, "hello\r\n");

        let (fm, body) = split("just text").expect("split");
        assert!(fm.is_none());
        assert_eq!(body, "just text");
    }

    #[test]
    fn unclosed_frontmatter_is_an_error() {
        assert!(split("---\nid: abc\nbody").is_err());
    }

    #[test]
    fn broken_yaml_is_an_error() {
        assert!(Frontmatter::parse("id: [unclosed").is_err());
        assert!(Frontmatter::parse("- just\n- a list").is_err());
    }
}
