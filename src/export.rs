//! Export a period as one self-contained HTML file.
//!
//! For sharing outside git: no account, no server, no JavaScript, no external
//! requests. Bodies are rendered as pre-formatted text rather than pretending to
//! be a Markdown renderer.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{DayDocError, Result};
use crate::ledger;
use crate::note::Note;
use crate::stale;
use crate::types::NoteType;
use crate::vault::Vault;

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub path: PathBuf,
    pub bytes: u64,
    pub notes: usize,
    pub decisions: usize,
}

impl ExportReport {
    pub fn render(&self) -> String {
        format!(
            "{}  {} note(s), {} decision(s), {} bytes\n",
            self.path.display(),
            self.notes,
            self.decisions,
            self.bytes
        )
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn note_date(note: &Note) -> String {
    for candidate in [
        note.frontmatter.decided_at.as_deref(),
        note.frontmatter.updated.as_deref(),
        note.frontmatter.created.as_deref(),
        note.frontmatter.last_verified.as_deref(),
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

fn in_range(date: &str, from: &str, to: &str) -> bool {
    !date.is_empty() && date >= from && date <= to
}

const STYLE: &str = "
body { font-family: -apple-system, Segoe UI, Roboto, sans-serif; margin: 0 auto;
       max-width: 46rem; padding: 3rem 1.5rem; color: #1a1a1a; line-height: 1.55; }
h1 { font-size: 1.9rem; margin-bottom: .2rem; }
h2 { font-size: 1.15rem; margin-top: 2.5rem; border-bottom: 1px solid #e4e4e4;
     padding-bottom: .35rem; }
.meta { color: #666; font-size: .9rem; margin-bottom: 2rem; }
.card { border: 1px solid #e4e4e4; border-radius: 6px; padding: .9rem 1rem;
        margin: 1rem 0; }
.card h3 { margin: 0 0 .35rem; font-size: 1rem; }
.tag { font-size: .78rem; text-transform: uppercase; letter-spacing: .04em;
       color: #555; }
pre { white-space: pre-wrap; font-family: ui-monospace, SFMono-Regular, Menlo,
      monospace; font-size: .87rem; background: #fafafa; padding: .8rem;
      border-radius: 4px; overflow-x: auto; }
footer { margin-top: 3rem; color: #777; font-size: .82rem; }
";

/// Write one HTML file covering `from`..`to` inclusive.
pub fn run(vault: &Vault, from: &str, to: &str, out: &Path) -> Result<ExportReport> {
    let notes = vault.scan()?.notes;
    let cards = ledger::cards(vault)?;
    let flags = stale::scan(vault)?;

    let mut decisions = String::new();
    let mut decision_count = 0usize;
    for card in &cards {
        let date = card.decided_at.clone().unwrap_or_default();
        if !in_range(&date, from, to) {
            continue;
        }
        decision_count += 1;
        decisions.push_str(&format!(
            "<div class=\"card\"><div class=\"tag\">{} · {}</div><h3>{}</h3>",
            escape(&card.decision_id),
            escape(card.status.as_str()),
            escape(&card.title)
        ));
        decisions.push_str(&format!(
            "<div class=\"meta\">{} · owner {} · supersedes {} · superseded by {}</div>",
            escape(&date),
            escape(card.owner.as_deref().unwrap_or("none")),
            escape(card.supersedes.as_deref().unwrap_or("none")),
            escape(card.superseded_by.as_deref().unwrap_or("none"))
        ));
        decisions.push_str("</div>\n");
    }

    let mut body = String::new();
    let mut note_count = 0usize;
    for note in &notes {
        if note.note_type() == NoteType::Decision {
            continue;
        }
        let date = note_date(note);
        if !in_range(&date, from, to) {
            continue;
        }
        note_count += 1;
        body.push_str(&format!(
            "<h2>{} <span class=\"tag\">{}</span></h2>\n",
            escape(&note.title()),
            escape(note.note_type().as_str())
        ));
        body.push_str(&format!(
            "<div class=\"meta\">{} · {}</div>\n",
            escape(&date),
            escape(&note.rel_path)
        ));
        body.push_str(&format!("<pre>{}</pre>\n", escape(note.body.trim())));
    }

    let mut stale_html = String::new();
    for flag in &flags {
        stale_html.push_str(&format!(
            "<li>{} <span class=\"tag\">{}</span><ul>{}</ul></li>\n",
            escape(&flag.title),
            escape(flag.rel_path.as_str()),
            flag.reasons
                .iter()
                .map(|reason| format!("<li>{}</li>", escape(&reason.describe())))
                .collect::<Vec<_>>()
                .join("")
        ));
    }

    let html = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>DayDoc export {from} to {to}</title><style>{STYLE}</style></head><body>\n\
         <h1>DayDoc</h1>\n<div class=\"meta\">{from} to {to} · {note_count} note(s) · \
         {decision_count} decision(s)</div>\n\
         <h2>Decisions</h2>\n{decisions}\
         <h2>Notes</h2>\n{body}\
         <h2>Stale at export time</h2>\n<ul>{stale_html}</ul>\n\
         <footer>Generated by DayDoc. Static file: no scripts, no external requests.</footer>\n\
         </body></html>\n"
    );

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DayDocError::io(parent, e))?;
    }
    crate::vault::atomic_write(out, html.as_bytes())?;
    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);

    Ok(ExportReport {
        path: out.to_path_buf(),
        bytes,
        notes: note_count,
        decisions: decision_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::NewDecision;
    use crate::vault::test_support::temp_vault;
    use crate::vault::today;

    #[test]
    fn export_is_one_self_contained_file() {
        let vault = temp_vault();
        let date = today();
        ledger::create(&vault, &NewDecision::new("Use Okta for SSO").accepted()).expect("decision");
        vault
            .write(
                "meetings/kickoff.md",
                &format!("---\nid: 01A\ntype: meeting\ntitle: Kickoff\ncreated: {date}\n---\nwe agreed to use Okta\n"),
            )
            .expect("note");

        let out = std::env::temp_dir().join(format!("daydoc-export-{}.html", crate::ids::new_ulid()));
        let report = run(&vault, &date, &date, &out).expect("export");
        assert_eq!(report.notes, 1);
        assert_eq!(report.decisions, 1);

        let html = std::fs::read_to_string(&out).expect("read");
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("Use Okta for SSO"));
        assert!(html.contains("Kickoff"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("http://"));
    }

    #[test]
    fn markup_in_a_note_is_escaped() {
        let vault = temp_vault();
        let date = today();
        vault
            .write(
                "inbox/x.md",
                &format!("---\nid: 01B\ntype: idea\ntitle: X\ncreated: {date}\n---\n<script>alert(1)</script>\n"),
            )
            .expect("note");
        let out = std::env::temp_dir().join(format!("daydoc-export-{}.html", crate::ids::new_ulid()));
        run(&vault, &date, &date, &out).expect("export");
        let html = std::fs::read_to_string(&out).expect("read");
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn a_period_with_nothing_in_it_still_exports() {
        let vault = temp_vault();
        let out = std::env::temp_dir().join(format!("daydoc-export-{}.html", crate::ids::new_ulid()));
        let report = run(&vault, "2020-01-01", "2020-01-31", &out).expect("export");
        assert_eq!(report.notes, 0);
        assert!(out.exists());
    }
}
