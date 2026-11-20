//! Import an existing Markdown folder: Obsidian, Logseq, or plain files.
//!
//! The folder is read, never modified. Paths are preserved so wikilinks keep
//! resolving; a missing `id` is assigned, a missing `type` is classified, and
//! nothing is overwritten. `preview` shows exactly what `run` would do.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::classify::classify;
use crate::error::{DayDocError, Result};
use crate::frontmatter;
use crate::ids;
use crate::note::Note;
use crate::types::NoteType;
use crate::vault::{now_iso, Vault};

const SKIP_DIRS: [&str; 8] = [
    "_meta", "_trash", "_archive", "assets", "node_modules", "target", "vendor", "dist",
];

#[derive(Debug, Clone, Serialize)]
pub struct ImportEntry {
    pub source: String,
    pub rel_path: String,
    pub note_type: NoteType,
    pub confidence: f32,
    pub conflict: bool,
    pub had_id: bool,
    pub had_type: bool,
}

impl ImportEntry {
    pub fn describe(&self) -> String {
        let mut flags = Vec::new();
        if self.conflict {
            flags.push("exists, will skip");
        }
        if !self.had_id {
            flags.push("new id");
        }
        if !self.had_type {
            flags.push("classified");
        }
        let flag = if flags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", flags.join(", "))
        };
        format!(
            "{} -> {} ({}, {:.0}%){}",
            self.source,
            self.rel_path,
            self.note_type,
            self.confidence * 100.0,
            flag
        )
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportReport {
    pub entries: Vec<ImportEntry>,
    pub written: usize,
    pub skipped: usize,
    pub broken: Vec<String>,
}

impl ImportReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            out.push_str(&entry.describe());
            out.push('\n');
        }
        let pending = self.entries.len().saturating_sub(self.skipped);
        let (count, verb) = if self.written == 0 {
            (pending, "to import")
        } else {
            (self.written, "imported")
        };
        out.push_str(&format!(
            "\n{count} note(s) {verb} · {} skipped · {} unreadable\n",
            self.skipped,
            self.broken.len()
        ));
        out
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| DayDocError::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| DayDocError::io(dir, e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // `symlink_metadata`, not `metadata`: a link inside the source folder must
        // not pull a file from outside it into the vault.
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            collect(&path, out)?;
        } else if meta.is_file() && name.ends_with(".md") && !name.starts_with('.') {
            out.push(path);
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// First real line of the note, so an untitled file keeps its meaning.
fn title_for(note: &Note) -> String {
    for line in note.body.lines() {
        let trimmed = line.trim().trim_start_matches('#').trim();
        if !trimmed.is_empty() {
            if trimmed.chars().count() > 90 {
                return trimmed.chars().take(87).collect::<String>() + "...";
            }
            return trimmed.to_string();
        }
    }
    note.title()
}

fn mtime_date(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let datetime: chrono::DateTime<chrono::Local> = chrono::DateTime::from(modified);
    Some(datetime.format("%Y-%m-%d").to_string())
}

/// What an import would do. Writes nothing.
pub fn preview(vault: &Vault, source: &Path) -> Result<ImportReport> {
    scan(vault, source, false)
}

/// Import for real.
pub fn run(vault: &Vault, source: &Path) -> Result<ImportReport> {
    scan(vault, source, true)
}

fn scan(vault: &Vault, source: &Path, write: bool) -> Result<ImportReport> {
    if !source.is_dir() {
        return Err(DayDocError::NoVault(format!(
            "{} is not a folder",
            source.display()
        )));
    }
    let mut files: Vec<PathBuf> = Vec::new();
    collect(source, &mut files)?;
    files.sort();

    let mut report = ImportReport::default();
    for path in files {
        let rel = relative(source, &path);
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => {
                report.broken.push(rel);
                continue;
            }
        };
        if frontmatter::split(&raw).is_err() {
            report.broken.push(rel);
            continue;
        }
        let mut note = match Note::parse(&rel, &raw) {
            Ok(note) => note,
            Err(_) => {
                report.broken.push(rel);
                continue;
            }
        };

        let had_id = !note.frontmatter.id.is_empty();
        let had_type = note.frontmatter.note_type().is_some();
        let classification = classify(&note.body);

        if !had_id {
            note.frontmatter.id = ids::new_ulid();
        }
        if !had_type {
            note.frontmatter.note_type = classification.note_type.as_str().to_string();
        }
        if note.frontmatter.title.is_empty() {
            note.frontmatter.title = title_for(&note);
        }
        if note.frontmatter.created.is_none() {
            note.frontmatter.created = mtime_date(&path);
        }
        note.frontmatter.updated = Some(now_iso());

        let conflict = vault.exists(&rel);
        if conflict {
            report.skipped += 1;
        } else if write {
            vault.create(&rel, &note.render())?;
            report.written += 1;
        }

        report.entries.push(ImportEntry {
            source: path.display().to_string(),
            rel_path: rel,
            note_type: note.note_type(),
            confidence: classification.confidence,
            conflict,
            had_id,
            had_type,
        });
    }

    if write {
        vault.audit(
            "import",
            &source.display().to_string(),
            &format!("{} written, {} skipped", report.written, report.skipped),
        )?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    fn source_tree() -> PathBuf {
        let root = std::env::temp_dir().join(format!("daydoc-import-{}", ids::new_ulid()));
        std::fs::create_dir_all(root.join("notes")).expect("dirs");
        std::fs::create_dir_all(root.join(".obsidian")).expect("dirs");
        std::fs::write(
            root.join("notes/plain.md"),
            "We decided to adopt Okta for SSO across all products.\n",
        )
        .expect("write");
        std::fs::write(
            root.join("notes/known.md"),
            "---\nid: 01ABC\ntype: idea\ntitle: Already mine\n---\nbody\n",
        )
        .expect("write");
        std::fs::write(root.join(".obsidian/config.md"), "hidden\n").expect("write");
        root
    }

    #[test]
    fn preview_reports_without_writing() {
        let vault = temp_vault();
        let source = source_tree();
        let report = preview(&vault, &source).expect("preview");
        assert_eq!(report.entries.len(), 2);
        assert_eq!(report.written, 0);
        assert!(!vault.exists("notes/plain.md"));
        assert!(report
            .entries
            .iter()
            .any(|e| e.note_type == NoteType::Decision && !e.had_id));
    }

    #[test]
    fn run_assigns_ids_and_classifies() {
        let vault = temp_vault();
        let source = source_tree();
        let report = run(&vault, &source).expect("import");
        assert_eq!(report.written, 2);

        let plain = vault.load("notes/plain.md").expect("plain");
        assert!(!plain.frontmatter.id.is_empty());
        assert_eq!(plain.note_type(), NoteType::Decision);
        assert_eq!(plain.title(), "We decided to adopt Okta for SSO across all products.");

        let known = vault.load("notes/known.md").expect("known");
        assert_eq!(known.frontmatter.id, "01ABC");
        assert_eq!(known.note_type(), NoteType::Idea);
        assert_eq!(known.title(), "Already mine");

        // The source folder is untouched.
        assert!(source.join("notes/plain.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_in_the_source_are_not_followed() {
        let vault = temp_vault();
        let outside = std::env::temp_dir().join(format!("daydoc-outside-{}.md", ids::new_ulid()));
        std::fs::write(&outside, "a secret outside the folder\n").expect("write outside");
        let source = std::env::temp_dir().join(format!("daydoc-symlink-{}", ids::new_ulid()));
        std::fs::create_dir_all(&source).expect("dirs");
        std::os::unix::fs::symlink(&outside, source.join("link.md")).expect("symlink");

        let report = run(&vault, &source).expect("import");
        assert!(report.entries.is_empty(), "followed a symlink: {report:?}");
        assert!(!vault.exists("link.md"));
    }

    #[test]
    fn a_second_import_does_not_clobber() {
        let vault = temp_vault();
        let source = source_tree();
        run(&vault, &source).expect("first");
        let report = run(&vault, &source).expect("second");
        assert_eq!(report.written, 0);
        assert_eq!(report.skipped, 2);
    }
}
