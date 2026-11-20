//! The vault: plain Markdown plus YAML on disk, and the only thing that writes
//! to it.
//!
//! Every write is staged in a write-ahead log, fsync'd, then committed by
//! rename. A crash between the two replays on next start. Nothing outside the
//! vault root can be addressed: relative paths only, no `..`, no absolute
//! paths, no symlink following on delete.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{DayDocError, Result};
use crate::ids;
use crate::note::Note;

pub const META: &str = "_meta";
pub const TRASH: &str = "_trash";
pub const ARCHIVE: &str = "_archive";
pub const ASSETS: &str = "assets";

/// Folders created by `daydoc init`. Everything else is the user's.
pub const VAULT_DIRS: [&str; 17] = [
    META,
    "_meta/warnings",
    "_meta/broken",
    TRASH,
    ARCHIVE,
    ASSETS,
    "inbox",
    "daily",
    "daily/digests",
    "decisions",
    "meetings",
    "ideas",
    "tasks",
    "reference",
    "sops",
    "projects",
    "handovers",
];

const SKIP_DIRS: [&str; 6] = [META, TRASH, ARCHIVE, ASSETS, "node_modules", "target"];

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
    state: PathBuf,
    config: Config,
}

#[derive(Debug, Clone)]
pub struct Scan {
    pub notes: Vec<Note>,
    /// Vault-relative paths of files whose frontmatter could not be read.
    pub broken: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WalHeader {
    target: String,
    len: u64,
    blake3: String,
}

#[derive(Debug, Serialize)]
struct AuditEntry<'a> {
    at: String,
    action: &'a str,
    subject: &'a str,
    detail: &'a str,
}

impl Vault {
    /// Create a vault at `root` if it is not one already. Idempotent.
    pub fn init(root: &Path) -> Result<Vault> {
        fs::create_dir_all(root).map_err(|e| DayDocError::io(root, e))?;
        for dir in VAULT_DIRS {
            let path = root.join(dir);
            fs::create_dir_all(&path).map_err(|e| DayDocError::io(&path, e))?;
        }
        let cfg_path = root.join(crate::config::CONFIG_FILE);
        let config = if !cfg_path.exists() {
            let fresh = Config {
                vault_root: root.display().to_string(),
                ..Config::default()
            };
            fresh.write_to(root)?;
            fresh
        } else {
            Config::load_from_vault(root)?
        };
        Ok(Vault {
            root: root.to_path_buf(),
            state: default_state_dir(),
            config,
        })
    }

    /// Open an existing vault. A folder that is not a vault yet is initialised,
    /// because "pick a folder" is the whole first-run story.
    pub fn open(root: &Path) -> Result<Vault> {
        if !root.exists() {
            return Err(DayDocError::NoVault(root.display().to_string()));
        }
        if !root.is_dir() {
            return Err(DayDocError::NoVault(format!(
                "{} is not a directory",
                root.display()
            )));
        }
        Vault::init(root)
    }

    /// Open with an explicit state directory (tests, portable installs).
    pub fn with_state(root: &Path, state: &Path) -> Result<Vault> {
        let mut vault = Vault::open(root)?;
        vault.state = state.to_path_buf();
        Ok(vault)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn state_dir(&self) -> &Path {
        &self.state
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    pub fn is_read_only(&self) -> bool {
        self.config.read_only_vault
    }

    pub fn wal_dir(&self) -> PathBuf {
        self.state.join("wal")
    }

    /// Resolve a vault-relative path, refusing anything that could escape.
    pub fn abs(&self, rel: &str) -> Result<PathBuf> {
        let candidate = Path::new(rel);
        if candidate.is_absolute() {
            return Err(DayDocError::UnsafePath(rel.to_string()));
        }
        let mut out = self.root.clone();
        for component in candidate.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(DayDocError::UnsafePath(rel.to_string()))
                }
            }
        }
        if !out.starts_with(&self.root) {
            return Err(DayDocError::UnsafePath(rel.to_string()));
        }
        Ok(out)
    }

    /// Vault-relative path with forward slashes, for storage and display.
    pub fn rel_of(&self, path: &Path) -> String {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        rel.components()
            .filter_map(|c| match c {
                Component::Normal(p) => Some(p.to_string_lossy().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    fn ensure_writable(&self, rel: &str) -> Result<()> {
        if self.is_read_only() {
            return Err(DayDocError::ReadOnly(rel.to_string()));
        }
        Ok(())
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.abs(rel).map(|p| p.exists()).unwrap_or(false)
    }

    pub fn read(&self, rel: &str) -> Result<String> {
        let path = self.abs(rel)?;
        fs::read_to_string(&path).map_err(|e| DayDocError::io(&path, e))
    }

    /// Write a file through the write-ahead log. Returns the absolute path.
    pub fn write(&self, rel: &str, content: &str) -> Result<PathBuf> {
        self.ensure_writable(rel)?;
        let target = self.abs(rel)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| DayDocError::io(parent, e))?;
        }

        let wal_dir = self.wal_dir();
        fs::create_dir_all(&wal_dir).map_err(|e| DayDocError::io(&wal_dir, e))?;
        let wal_path = wal_dir.join(format!("{}.wal", ids::new_ulid()));
        let header = WalHeader {
            target: rel.to_string(),
            len: content.len() as u64,
            blake3: blake3::hash(content.as_bytes()).to_hex().to_string(),
        };
        let mut payload = String::with_capacity(content.len() + 128);
        payload.push_str(
            &serde_json::to_string(&header)
                .map_err(|e| DayDocError::Ledger(format!("wal header: {e}")))?,
        );
        payload.push('\n');
        payload.push_str(content);
        {
            let mut file =
                fs::File::create(&wal_path).map_err(|e| DayDocError::io(&wal_path, e))?;
            file.write_all(payload.as_bytes())
                .map_err(|e| DayDocError::io(&wal_path, e))?;
            file.sync_all().map_err(|e| DayDocError::io(&wal_path, e))?;
        }

        atomic_write(&target, content.as_bytes())?;
        let _ = fs::remove_file(&wal_path);
        Ok(target)
    }

    /// Write raw bytes through the same path (attachments are not text).
    pub fn write_bytes(&self, rel: &str, bytes: &[u8]) -> Result<PathBuf> {
        self.ensure_writable(rel)?;
        let target = self.abs(rel)?;
        atomic_write(&target, bytes)?;
        Ok(target)
    }

    /// Write a new file, refusing to clobber an existing note.
    pub fn create(&self, rel: &str, content: &str) -> Result<PathBuf> {
        if self.exists(rel) {
            return Err(DayDocError::Ledger(format!("{rel} already exists")));
        }
        self.write(rel, content)
    }

    pub fn save(&self, note: &Note) -> Result<PathBuf> {
        self.write(&note.rel_path, &note.render())
    }

    /// Load a note. Broken frontmatter is quarantined to `_meta/broken/` and
    /// reported, and the original file is never modified.
    pub fn load(&self, rel: &str) -> Result<Note> {
        let raw = self.read(rel)?;
        match Note::parse(rel, &raw) {
            Ok(note) => Ok(note),
            Err(err) => {
                self.quarantine_broken(rel, &raw);
                Err(DayDocError::BrokenFrontmatter {
                    path: rel.to_string(),
                    reason: err.to_string(),
                })
            }
        }
    }

    fn quarantine_broken(&self, rel: &str, raw: &str) {
        if self.is_read_only() {
            return;
        }
        let stem = ids::slug(rel.rsplit('/').next().unwrap_or(rel), 60);
        // Named by content, not by time: scanning the same broken note twice must
        // not leave two copies behind.
        let digest = blake3::hash(raw.as_bytes()).to_hex().to_string();
        let dest = format!("{META}/broken/{stem}-{}.md", &digest[..8]);
        let _ = self.write(&dest, raw);
        let _ = self.audit("frontmatter-broken", rel, "quarantined to _meta/broken/");
    }

    /// Every note in the vault, skipping `_trash`, `_archive`, `_meta`,
    /// `assets`, dot-directories and build output.
    pub fn scan(&self) -> Result<Scan> {
        let mut paths: Vec<PathBuf> = Vec::new();
        collect_md(&self.root, &self.root, &mut paths)?;
        paths.sort();

        let mut notes = Vec::new();
        let mut broken = Vec::new();
        for path in paths {
            let rel = self.rel_of(&path);
            match self.load(&rel) {
                Ok(note) => notes.push(note),
                Err(DayDocError::BrokenFrontmatter { .. }) => broken.push(rel),
                Err(DayDocError::Io { .. }) => broken.push(rel),
                Err(other) => return Err(other),
            }
        }
        Ok(Scan { notes, broken })
    }

    pub fn list_notes(&self) -> Result<Vec<Note>> {
        Ok(self.scan()?.notes)
    }

    pub fn list_rels(&self) -> Result<Vec<String>> {
        let mut paths: Vec<PathBuf> = Vec::new();
        collect_md(&self.root, &self.root, &mut paths)?;
        let mut out: Vec<String> = paths.iter().map(|p| self.rel_of(p)).collect();
        out.sort();
        Ok(out)
    }

    /// Move a note to `_trash/`. Never follows symlinks, never leaves the vault.
    pub fn trash(&self, rel: &str) -> Result<String> {
        self.ensure_writable(rel)?;
        let src = self.abs(rel)?;
        let meta = fs::symlink_metadata(&src).map_err(|e| DayDocError::io(&src, e))?;
        if meta.file_type().is_symlink() {
            return Err(DayDocError::UnsafePath(format!(
                "{rel} is a symlink; refusing to move it"
            )));
        }
        let root_canonical = self
            .root
            .canonicalize()
            .map_err(|e| DayDocError::io(&self.root, e))?;
        let canonical = src.canonicalize().map_err(|e| DayDocError::io(&src, e))?;
        if !canonical.starts_with(&root_canonical) {
            return Err(DayDocError::UnsafePath(rel.to_string()));
        }

        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "note.md".to_string());
        let dest_rel = format!("{TRASH}/{}-{}", ids::new_ulid(), name);
        let dest = self.abs(&dest_rel)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| DayDocError::io(parent, e))?;
        }
        fs::rename(&src, &dest).map_err(|e| DayDocError::io(&src, e))?;
        self.audit("trash", rel, &dest_rel)?;
        Ok(dest_rel)
    }

    /// Move a note to `_archive/YYYY/`.
    pub fn archive(&self, rel: &str) -> Result<String> {
        self.ensure_writable(rel)?;
        let src = self.abs(rel)?;
        let year = chrono::Local::now().format("%Y").to_string();
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "note.md".to_string());
        let dest_rel = format!("{ARCHIVE}/{year}/{name}");
        let dest = self.abs(&dest_rel)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| DayDocError::io(parent, e))?;
        }
        fs::rename(&src, &dest).map_err(|e| DayDocError::io(&src, e))?;
        self.audit("archive", rel, &dest_rel)?;
        Ok(dest_rel)
    }

    /// Purge trash entries older than the retention window. Returns how many
    /// files went.
    pub fn purge_trash(&self) -> Result<usize> {
        if self.is_read_only() {
            return Ok(0);
        }
        let dir = self.root.join(TRASH);
        if !dir.exists() {
            return Ok(0);
        }
        let retention = chrono::Duration::days(self.config.trash_retention_days.max(0));
        let cutoff = chrono::Local::now() - retention;
        let mut removed = 0usize;
        for entry in fs::read_dir(&dir).map_err(|e| DayDocError::io(&dir, e))? {
            let entry = entry.map_err(|e| DayDocError::io(&dir, e))?;
            let path = entry.path();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() || meta.file_type().is_symlink() {
                continue;
            }
            let modified: chrono::DateTime<chrono::Local> = match meta.modified() {
                Ok(t) => chrono::DateTime::<chrono::Local>::from(t),
                Err(_) => continue,
            };
            if modified < cutoff {
                if fs::remove_file(&path).is_ok() {
                    removed += 1;
                    let rel = self.rel_of(&path);
                    let _ = self.audit("trash-purge", &rel, "past retention window");
                }
            }
        }
        Ok(removed)
    }

    /// Append one line to `_meta/audit.jsonl`.
    pub fn audit(&self, action: &str, subject: &str, detail: &str) -> Result<()> {
        if self.is_read_only() {
            return Ok(());
        }
        let entry = AuditEntry {
            at: now_iso(),
            action,
            subject,
            detail,
        };
        let line = serde_json::to_string(&entry)
            .map_err(|e| DayDocError::Ledger(format!("audit entry: {e}")))?;
        let rel = format!("{META}/audit.jsonl");
        let path = self.abs(&rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| DayDocError::io(parent, e))?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| DayDocError::io(&path, e))?;
        writeln!(file, "{line}").map_err(|e| DayDocError::io(&path, e))?;
        file.sync_all().map_err(|e| DayDocError::io(&path, e))?;
        Ok(())
    }

    /// The counters and index database. Lives in the state directory, not the
    /// vault: the vault rebuilds it, so losing it costs nothing.
    pub fn db(&self) -> Result<Connection> {
        fs::create_dir_all(&self.state).map_err(|e| DayDocError::io(&self.state, e))?;
        let path = self.state.join("index.db");
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS counters (
                 name TEXT PRIMARY KEY,
                 value INTEGER NOT NULL
             );",
        )?;
        Ok(conn)
    }

    /// Replay any staged writes left behind by a crash. Returns human-readable
    /// warnings; a corrupt log entry is moved aside rather than applied.
    pub fn replay_wal(&self) -> Result<Vec<String>> {
        let wal_dir = self.wal_dir();
        let mut warnings = Vec::new();
        if !wal_dir.exists() {
            return Ok(warnings);
        }
        if self.is_read_only() {
            warnings.push(
                "read-only vault: staged writes were not replayed".to_string(),
            );
            return Ok(warnings);
        }
        let mut entries: Vec<PathBuf> = Vec::new();
        for entry in fs::read_dir(&wal_dir).map_err(|e| DayDocError::io(&wal_dir, e))? {
            let entry = entry.map_err(|e| DayDocError::io(&wal_dir, e))?;
            let path = entry.path();
            if path.extension().map(|e| e == "wal").unwrap_or(false) {
                entries.push(path);
            }
        }
        entries.sort();

        for path in entries {
            let raw = match fs::read_to_string(&path) {
                Ok(r) => r,
                Err(e) => {
                    warnings.push(format!("wal {} unreadable: {e}", path.display()));
                    continue;
                }
            };
            let (header_line, body) = match raw.split_once('\n') {
                Some(parts) => parts,
                None => {
                    quarantine(&path);
                    warnings.push(format!(
                        "wal {} had no header; moved aside, vault untouched",
                        path.display()
                    ));
                    continue;
                }
            };
            let header: WalHeader = match serde_json::from_str(header_line) {
                Ok(h) => h,
                Err(_) => {
                    quarantine(&path);
                    warnings.push(format!(
                        "wal {} header unparseable; moved aside, vault untouched",
                        path.display()
                    ));
                    continue;
                }
            };
            let ok = body.len() as u64 == header.len
                && blake3::hash(body.as_bytes()).to_hex().to_string() == header.blake3;
            if !ok {
                quarantine(&path);
                warnings.push(format!(
                    "wal {} failed its checksum; replay aborted, vault untouched",
                    path.display()
                ));
                continue;
            }
            let target = match self.abs(&header.target) {
                Ok(t) => t,
                Err(_) => {
                    quarantine(&path);
                    warnings.push(format!(
                        "wal {} targets a path outside the vault; moved aside",
                        path.display()
                    ));
                    continue;
                }
            };
            match atomic_write(&target, body.as_bytes()) {
                Ok(()) => {
                    let _ = fs::remove_file(&path);
                    warnings.push(format!("replayed staged write to {}", header.target));
                }
                Err(e) => warnings.push(format!(
                    "could not replay {}: {e}",
                    header.target
                )),
            }
        }
        Ok(warnings)
    }

    pub fn note_count(&self) -> Result<usize> {
        Ok(self.list_rels()?.len())
    }
}

/// Today's date as `YYYY-MM-DD`, local time.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Now as `YYYY-MM-DDTHH:MM:SSZ` in UTC.
pub fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn default_state_dir() -> PathBuf {
    for key in ["DAYDOC_HOME"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return PathBuf::from(value);
            }
        }
    }
    for key in ["HOME", "USERPROFILE"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return PathBuf::from(value).join(".daydoc");
            }
        }
    }
    PathBuf::from(".daydoc")
}

fn quarantine(path: &Path) {
    let dir = path.parent().map(|p| p.join("rejected"));
    if let Some(dir) = dir {
        if fs::create_dir_all(&dir).is_ok() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "entry.wal".to_string());
            let _ = fs::rename(path, dir.join(name));
        }
    }
}

/// Write bytes to `path` via a temporary file in the same directory, then
/// rename. On Windows, sharing violations (os error 32/33) are retried with
/// 1→16 s backoff, because an editor holding the file is normal, not fatal.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = match path.parent() {
        Some(p) => p,
        None => return Err(DayDocError::UnsafePath(path.display().to_string())),
    };
    fs::create_dir_all(parent).map_err(|e| DayDocError::io(parent, e))?;
    let tmp = parent.join(format!(".daydoc-tmp-{}", ids::new_ulid()));
    {
        let mut file = fs::File::create(&tmp).map_err(|e| DayDocError::io(&tmp, e))?;
        file.write_all(bytes).map_err(|e| DayDocError::io(&tmp, e))?;
        file.sync_all().map_err(|e| DayDocError::io(&tmp, e))?;
    }
    let mut attempt = 0u32;
    let mut backoff = 1u64;
    loop {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(err) if cfg!(windows) && is_sharing_violation(&err) && attempt < 5 => {
                std::thread::sleep(std::time::Duration::from_secs(backoff));
                attempt += 1;
                backoff = (backoff * 2).min(16);
            }
            Err(err) => {
                let _ = fs::remove_file(&tmp);
                return Err(DayDocError::io(path, err));
            }
        }
    }
}

fn is_sharing_violation(err: &std::io::Error) -> bool {
    matches!(err.raw_os_error(), Some(32) | Some(33))
}

fn collect_md(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => return Err(DayDocError::io(dir, e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| DayDocError::io(dir, e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if path == root.join(META) {
                continue;
            }
            collect_md(root, &path, out)?;
        } else if meta.is_file() && name.ends_with(".md") && !name.starts_with('.') {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A throwaway vault plus state directory. Unique per call.
    pub fn temp_vault() -> Vault {
        let base = std::env::temp_dir().join(format!("daydoc-test-{}", ids::new_ulid()));
        let root = base.join("vault");
        let state = base.join("state");
        fs::create_dir_all(&root).expect("create temp vault");
        Vault::with_state(&root, &state).expect("open temp vault")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::temp_vault;
    use super::*;

    #[test]
    fn init_creates_folders_and_config() {
        let vault = temp_vault();
        for dir in VAULT_DIRS {
            assert!(vault.root().join(dir).is_dir(), "missing {dir}");
        }
        assert!(vault.root().join(crate::config::CONFIG_FILE).is_file());
        assert_eq!(vault.config().llm_mode, "off");
    }

    #[test]
    fn paths_cannot_escape_the_vault() {
        let vault = temp_vault();
        assert!(vault.abs("../escape.md").is_err());
        assert!(vault.abs("/etc/passwd").is_err());
        assert!(vault.abs("decisions/DR-0001.md").is_ok());
        assert!(vault.abs("./inbox/a.md").is_ok());
    }

    #[test]
    fn write_read_round_trip_leaves_no_wal() {
        let vault = temp_vault();
        vault.write("inbox/a.md", "hello").expect("write");
        assert_eq!(vault.read("inbox/a.md").expect("read"), "hello");
        let leftover: Vec<_> = fs::read_dir(vault.wal_dir())
            .map(|d| d.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();
        assert!(leftover.is_empty(), "wal not cleaned up");
    }

    #[test]
    fn read_only_vault_refuses_writes() {
        let mut vault = temp_vault();
        vault.config_mut().read_only_vault = true;
        let err = vault.write("inbox/a.md", "x").expect_err("should refuse");
        assert!(matches!(err, DayDocError::ReadOnly(_)));
    }

    #[test]
    fn create_refuses_to_clobber() {
        let vault = temp_vault();
        vault.create("inbox/a.md", "one").expect("create");
        assert!(vault.create("inbox/a.md", "two").is_err());
    }

    #[test]
    fn wal_replay_commits_a_staged_write() {
        let vault = temp_vault();
        fs::create_dir_all(vault.wal_dir()).expect("wal dir");
        let content = "recovered\n";
        let header = WalHeader {
            target: "inbox/recovered.md".to_string(),
            len: content.len() as u64,
            blake3: blake3::hash(content.as_bytes()).to_hex().to_string(),
        };
        let payload = format!("{}\n{}", serde_json::to_string(&header).unwrap(), content);
        fs::write(vault.wal_dir().join("0001.wal"), payload).expect("write wal");

        let warnings = vault.replay_wal().expect("replay");
        assert!(warnings.iter().any(|w| w.contains("replayed")));
        assert_eq!(vault.read("inbox/recovered.md").expect("read"), content);
    }

    #[test]
    fn corrupt_wal_is_quarantined_not_applied() {
        let vault = temp_vault();
        fs::create_dir_all(vault.wal_dir()).expect("wal dir");
        fs::write(vault.wal_dir().join("0002.wal"), "not json\nbody").expect("write wal");
        let warnings = vault.replay_wal().expect("replay");
        assert!(warnings.iter().any(|w| w.contains("moved aside")));
        assert!(!vault.exists("inbox/body"));
    }

    #[test]
    fn broken_frontmatter_is_quarantined_and_reported() {
        let vault = temp_vault();
        vault
            .write("inbox/broken.md", "---\nid: [unclosed\n---\nbody\n")
            .expect("write");
        let err = vault.load("inbox/broken.md").expect_err("should fail");
        assert!(matches!(err, DayDocError::BrokenFrontmatter { .. }));
        // Original untouched.
        assert!(vault
            .read("inbox/broken.md")
            .expect("read")
            .contains("unclosed"));
        // Copy exists under _meta/broken.
        let broken_dir = vault.root().join("_meta/broken");
        let count = fs::read_dir(&broken_dir).map(|d| d.count()).unwrap_or(0);
        assert!(count >= 1);
    }

    #[test]
    fn a_broken_note_is_quarantined_once_not_once_per_scan() {
        let vault = temp_vault();
        vault
            .write("inbox/broken.md", "---\nid: [unclosed\n---\nbody\n")
            .expect("write");
        for _ in 0..3 {
            let _ = vault.load("inbox/broken.md");
            let _ = vault.scan();
        }
        let count = fs::read_dir(vault.root().join("_meta/broken"))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(count, 1, "quarantine copies accumulated");
    }

    #[test]
    fn scan_skips_trash_and_meta() {
        let vault = temp_vault();
        vault
            .write("inbox/keep.md", "---\ntype: idea\ntitle: Keep\n---\nhi\n")
            .expect("write");
        vault
            .write("_trash/old.md", "---\ntype: idea\n---\nold\n")
            .expect("write");
        vault
            .write("_meta/audit.jsonl", "{}\n")
            .expect("write");
        let scan = vault.scan().expect("scan");
        assert_eq!(scan.notes.len(), 1);
        assert_eq!(scan.notes[0].title(), "Keep");
    }

    #[test]
    fn trash_and_archive_move_files() {
        let vault = temp_vault();
        vault.write("inbox/a.md", "a").expect("write");
        let dest = vault.trash("inbox/a.md").expect("trash");
        assert!(dest.starts_with("_trash/"));
        assert!(vault.abs(&dest).expect("abs").exists());

        vault.write("inbox/b.md", "b").expect("write");
        let dest = vault.archive("inbox/b.md").expect("archive");
        assert!(dest.starts_with("_archive/"));
        assert!(vault.abs(&dest).expect("abs").exists());
    }

    #[test]
    fn audit_log_is_append_only_jsonl() {
        let vault = temp_vault();
        vault.audit("test", "DR-0001", "detail").expect("audit");
        vault.audit("test", "DR-0002", "detail").expect("audit");
        let raw = vault.read("_meta/audit.jsonl").expect("read");
        assert_eq!(raw.lines().count(), 2);
        let first: serde_json::Value = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
        assert_eq!(first["subject"], "DR-0001");
    }
}
