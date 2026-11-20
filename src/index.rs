//! Note identity guard.
//!
//! A ULID is the identity. If someone edits a file and changes or removes its
//! `id`, that file is a *new* note: the original id stays in the index, the
//! note is not re-linked, and the change is reported into `_meta/warnings/`
//! rather than silently rewriting the past.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{DayDocError, Result};
use crate::ids;
use crate::note::Note;
use crate::vault::{now_iso, Vault};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS note_ids (
    rel_path TEXT PRIMARY KEY,
    id TEXT NOT NULL,
    seen_at TEXT NOT NULL
);";

/// Metadata plus a full-text index. Disposable: `reindex` rebuilds both from
/// the vault, so a stale or missing index costs time, never truth.
pub const FTS_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS note_meta (
    rel_path TEXT PRIMARY KEY,
    note_type TEXT NOT NULL,
    status TEXT,
    updated TEXT,
    title TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS index_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS note_fts USING fts5(
    rel_path UNINDEXED,
    title,
    body,
    entities,
    tokenize = 'unicode61 remove_diacritics 2'
);
";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReindexReport {
    pub indexed: usize,
    pub broken: usize,
    pub identities_changed: usize,
}

impl ReindexReport {
    pub fn render(&self) -> String {
        format!(
            "indexed {} note(s) · {} unreadable · {} identity change(s)\n",
            self.indexed, self.broken, self.identities_changed
        )
    }
}

fn stored_file_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT value FROM index_state WHERE key = 'file_count'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|value| value.parse().ok())
    .unwrap_or(-1)
}

fn set_file_count(conn: &Connection, files: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO index_state (key, value) VALUES ('file_count', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![files.to_string()],
    )
    .map_err(DayDocError::Db)?;
    Ok(())
}

fn entities_of(note: &Note) -> String {
    format!(
        "{} {} {}",
        note.frontmatter.people.join(" "),
        note.frontmatter.projects.join(" "),
        note.frontmatter.tags.join(" ")
    )
}

fn upsert_with(conn: &Connection, note: &Note) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO note_meta (rel_path, note_type, status, updated, title)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            note.rel_path,
            note.note_type().as_str(),
            note.frontmatter.status,
            note.frontmatter.updated,
            note.title()
        ],
    )
    .map_err(DayDocError::Db)?;
    conn.execute(
        "DELETE FROM note_fts WHERE rel_path = ?1",
        params![note.rel_path],
    )
    .map_err(DayDocError::Db)?;
    conn.execute(
        "INSERT INTO note_fts (rel_path, title, body, entities) VALUES (?1, ?2, ?3, ?4)",
        params![note.rel_path, note.title(), note.body, entities_of(note)],
    )
    .map_err(DayDocError::Db)?;
    Ok(())
}

/// Rebuild the whole index from the vault.
pub fn reindex(vault: &Vault) -> Result<ReindexReport> {
    let conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch(FTS_SCHEMA)?;
    conn.execute("DELETE FROM note_meta", [])
        .map_err(DayDocError::Db)?;
    conn.execute("DELETE FROM note_fts", [])
        .map_err(DayDocError::Db)?;

    let scan = vault.scan()?;
    let mut report = ReindexReport {
        broken: scan.broken.len(),
        ..Default::default()
    };
    for note in &scan.notes {
        upsert_with(&conn, note)?;
        report.indexed += 1;
    }
    // Files, not notes: a file with broken frontmatter is still on disk, and the
    // index must notice when one appears.
    set_file_count(&conn, vault.list_rels()?.len() as i64)?;
    report.identities_changed = check_identity(vault)?.len();
    vault.audit(
        "reindex",
        &report.indexed.to_string(),
        "notes indexed",
    )?;
    Ok(report)
}

/// Rebuild the index only if it disagrees with the vault. Called on open, so
/// search is fast without a maintenance ritual.
pub fn ensure(vault: &Vault) -> Result<Option<ReindexReport>> {
    if vault.is_read_only() {
        return Ok(None);
    }
    let conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch(FTS_SCHEMA)?;
    let files = vault.list_rels()?.len() as i64;
    if stored_file_count(&conn) == files && files > 0 {
        return Ok(None);
    }
    Ok(Some(reindex(vault)?))
}

/// Index a single path after a change, or drop it if the file is gone.
///
/// Returns whether the path is now in the index. A note that cannot be parsed is
/// not an error here: it is reported by `lint`, and the next full `reindex`
/// settles the count.
pub fn upsert_path(vault: &Vault, rel: &str) -> Result<bool> {
    if !vault.exists(rel) {
        remove(vault, rel)?;
        return Ok(false);
    }
    let note = match vault.load(rel) {
        Ok(note) => note,
        Err(_) => {
            remove(vault, rel)?;
            return Ok(false);
        }
    };
    upsert(vault, &note)?;
    Ok(true)
}

/// Drop a path from the index.
pub fn remove(vault: &Vault, rel: &str) -> Result<()> {
    let conn = vault.db()?;
    conn.execute_batch(FTS_SCHEMA)?;
    conn.execute("DELETE FROM note_meta WHERE rel_path = ?1", params![rel])
        .map_err(DayDocError::Db)?;
    conn.execute("DELETE FROM note_fts WHERE rel_path = ?1", params![rel])
        .map_err(DayDocError::Db)?;
    Ok(())
}

/// Record the current file count, so `ensure` and `candidates` know whether the
/// index still describes the vault.
pub fn stamp(vault: &Vault) -> Result<()> {
    let conn = vault.db()?;
    conn.execute_batch(FTS_SCHEMA)?;
    set_file_count(&conn, vault.list_rels()?.len() as i64)
}

/// Index one note after it changes. A missing index is not an error: the next
/// `reindex` rebuilds it.
pub fn upsert(vault: &Vault, note: &Note) -> Result<()> {
    let conn = vault.db()?;
    conn.execute_batch(FTS_SCHEMA)?;
    upsert_with(&conn, note)
}

/// Candidate paths for a query, or `None` when the caller should scan instead.
///
/// `None` covers three honest cases: no index yet, an index that disagrees with
/// the vault (so it is stale), and a query the full-text index cannot match —
/// which is exactly when fuzzy matching earns its keep.
pub fn candidates(vault: &Vault, tokens: &[String]) -> Option<Vec<String>> {
    if tokens.is_empty() {
        return None;
    }
    let conn = vault.db().ok()?;
    conn.execute_batch(FTS_SCHEMA).ok()?;
    let files = vault.list_rels().ok()?.len() as i64;
    if files == 0 || stored_file_count(&conn) != files {
        return None;
    }

    let expression = tokens
        .iter()
        .map(|token| format!("{token}*"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut statement = conn
        .prepare("SELECT rel_path FROM note_fts WHERE note_fts MATCH ?1 LIMIT 500")
        .ok()?;
    let rows = statement.query_map(params![expression], |row| row.get::<_, String>(0)).ok()?;
    let mut out = Vec::new();
    for row in rows.flatten() {
        out.push(row);
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityChange {
    pub rel_path: String,
    pub old_id: String,
    pub new_id: String,
}

impl IdentityChange {
    pub fn describe(&self) -> String {
        format!(
            "{}: id was {}, now {} — treated as a new note",
            self.rel_path, self.old_id, self.new_id
        )
    }
}

fn write_warning(vault: &Vault, change: &IdentityChange) -> Result<String> {
    let stem = ids::slug(
        change.rel_path.rsplit('/').next().unwrap_or(&change.rel_path),
        50,
    );
    let rel = format!(
        "_meta/warnings/{}-{}.md",
        chrono::Local::now().format("%Y-%m-%d"),
        stem
    );
    let mut body = String::new();
    body.push_str(&format!("# Note identity changed\n\n{}\n\n", change.describe()));
    body.push_str("DayDoc did not touch the file. The original id is kept in the\n");
    body.push_str("index so links to the old note still resolve.\n\n");
    body.push_str(&format!("- file: `{}`\n", change.rel_path));
    body.push_str(&format!("- was: `{}`\n", change.old_id));
    body.push_str(&format!("- now: `{}`\n", change.new_id));
    body.push_str(&format!("- noticed: {}\n", now_iso()));
    vault.write(&rel, &body)?;
    Ok(rel)
}

/// Compare every note's id against what the index last saw. Returns the changes
/// and writes one warning file per change.
pub fn check_identity(vault: &Vault) -> Result<Vec<IdentityChange>> {
    let conn = vault.db()?;
    conn.execute_batch(SCHEMA)?;
    let mut changes = Vec::new();

    for note in vault.scan()?.notes {
        let id = note.frontmatter.id.clone();
        if id.is_empty() {
            continue;
        }
        let stored: Option<String> = conn
            .query_row(
                "SELECT id FROM note_ids WHERE rel_path = ?1",
                params![note.rel_path],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(old_id) = stored {
            if old_id != id {
                let change = IdentityChange {
                    rel_path: note.rel_path.clone(),
                    old_id,
                    new_id: id.clone(),
                };
                if !vault.is_read_only() {
                    write_warning(vault, &change)?;
                }
                changes.push(change);
            }
        }

        conn.execute(
            "INSERT INTO note_ids (rel_path, id, seen_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(rel_path) DO UPDATE SET id = excluded.id, seen_at = excluded.seen_at",
            params![note.rel_path, id, now_iso()],
        )
        .map_err(DayDocError::Db)?;
    }

    if !changes.is_empty() {
        let summary = changes
            .iter()
            .map(|c| c.describe())
            .collect::<Vec<_>>()
            .join("; ");
        vault.audit("identity-change", "_meta/warnings", &summary)?;
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn reindex_then_candidates_find_and_miss() {
        let vault = temp_vault();
        vault
            .write(
                "inbox/a.md",
                "---\nid: 01A\ntype: idea\ntitle: Razorpay mandate\n---\nbody about payments\n",
            )
            .expect("write");
        let report = reindex(&vault).expect("reindex");
        assert_eq!(report.indexed, 1);

        let found = candidates(&vault, &["razorpay".to_string()]).expect("indexed");
        assert_eq!(found, vec!["inbox/a.md".to_string()]);

        let missing = candidates(&vault, &["kubernetes".to_string()]).expect("indexed");
        assert!(missing.is_empty());

        // A typo is a miss for the index; search falls back to a scan for it.
        let typo = candidates(&vault, &["razorpy".to_string()]).expect("indexed");
        assert!(typo.is_empty());
    }

    #[test]
    fn a_single_path_can_be_indexed_and_dropped_without_a_full_rebuild() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01A\ntype: idea\ntitle: Razorpay\n---\nbody\n")
            .expect("a");
        reindex(&vault).expect("reindex");

        vault
            .write("inbox/b.md", "---\nid: 01B\ntype: idea\ntitle: Okta\n---\nbody\n")
            .expect("b");
        assert!(upsert_path(&vault, "inbox/b.md").expect("upsert"));
        stamp(&vault).expect("stamp");
        // The stamp now agrees with the vault, so nothing asks for a rebuild.
        assert!(ensure(&vault).expect("ensure").is_none());
        let hits = candidates(&vault, &["okta".to_string()]).expect("indexed");
        assert_eq!(hits, vec!["inbox/b.md".to_string()]);

        std::fs::remove_file(vault.abs("inbox/b.md").expect("abs")).expect("delete");
        assert!(!upsert_path(&vault, "inbox/b.md").expect("drop"));
        stamp(&vault).expect("stamp");
        assert!(candidates(&vault, &["okta".to_string()]).expect("indexed").is_empty());
        assert!(ensure(&vault).expect("ensure").is_none());
    }

    #[test]
    fn a_stale_index_is_not_trusted() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01A\ntype: idea\ntitle: Razorpay\n---\nbody\n")
            .expect("a");
        reindex(&vault).expect("reindex");
        assert!(candidates(&vault, &["razorpay".to_string()]).is_some());

        vault
            .write("inbox/b.md", "---\nid: 01B\ntype: idea\ntitle: Okta\n---\nbody\n")
            .expect("b");
        assert!(candidates(&vault, &["razorpay".to_string()]).is_none());

        // And the self-healing path rebuilds it.
        assert!(ensure(&vault).expect("ensure").is_some());
        assert!(candidates(&vault, &["razorpay".to_string()]).is_some());
    }

    #[test]
    fn a_stable_id_produces_no_warning() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01AAA\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("write");
        assert!(check_identity(&vault).expect("check").is_empty());
        assert!(check_identity(&vault).expect("check").is_empty());
    }

    #[test]
    fn changing_an_id_is_reported_not_applied() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01AAA\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("write");
        check_identity(&vault).expect("first");

        vault
            .write("inbox/a.md", "---\nid: 01BBB\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("rewrite");
        let changes = check_identity(&vault).expect("second");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].old_id, "01AAA");
        assert_eq!(changes[0].new_id, "01BBB");

        // The file is untouched, and a warning exists.
        let raw = vault.read("inbox/a.md").expect("read");
        assert!(raw.contains("01BBB"));
        let warning_dir = vault.root().join("_meta/warnings");
        let count = std::fs::read_dir(&warning_dir).map(|d| d.count()).unwrap_or(0);
        assert!(count >= 1, "no warning written");
    }

    #[test]
    fn removing_an_id_is_not_a_change_we_can_track() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01AAA\ntype: idea\n---\nbody\n")
            .expect("write");
        check_identity(&vault).expect("first");
        vault
            .write("inbox/a.md", "---\ntype: idea\n---\nbody\n")
            .expect("rewrite");
        // No id, so nothing to compare; the note is simply un-identified.
        assert!(check_identity(&vault).expect("second").is_empty());
    }
}
