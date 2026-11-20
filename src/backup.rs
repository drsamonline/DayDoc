//! Backups: a dated `.tar.gz` of the vault, with 7 daily / 4 weekly / 6 monthly
//! retention.
//!
//! Only the vault is archived — the index rebuilds from it, so a backup is a
//! backup of the thing that matters. Backups land in the state directory, not
//! inside the vault being backed up.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Duration, NaiveDate};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::{Archive, Builder};

use crate::error::{DayDocError, Result};
use crate::secret;
use crate::vault::Vault;
use crate::{crypto, ids};

pub const PREFIX: &str = "daydoc-";
pub const SUFFIX: &str = ".tar.gz";
pub const ENCRYPTED_SUFFIX: &str = ".tar.gz.enc";
pub const DAILY_KEEP: usize = 7;
pub const WEEKLY_KEEP: usize = 4;
pub const MONTHLY_KEEP: usize = 6;

/// Directories that never belong in a backup of the vault.
const SKIP_DIRS: [&str; 3] = ["_trash", "backups", "node_modules"];

#[derive(Debug, Clone)]
pub struct BackupReport {
    pub path: PathBuf,
    pub bytes: u64,
    pub files: usize,
    pub pruned: Vec<String>,
    pub kept: usize,
    pub encrypted: bool,
}

impl BackupReport {
    pub fn render(&self) -> String {
        let mut out = format!(
            "{}  {} file(s), {} bytes{}\n",
            self.path.display(),
            self.files,
            self.bytes,
            if self.encrypted { ", AES-256-GCM" } else { "" }
        );
        out.push_str(&format!("{} backup(s) kept after retention\n", self.kept));
        for pruned in &self.pruned {
            out.push_str(&format!("  pruned {pruned}\n"));
        }
        out
    }
}

fn add_tree<W: std::io::Write>(
    builder: &mut Builder<W>,
    root: &Path,
    dir: &Path,
    files: &mut usize,
) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| DayDocError::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| DayDocError::io(dir, e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) || name == ".git" {
                continue;
            }
            add_tree(builder, root, &path, files)?;
        } else if meta.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            builder
                .append_path_with_name(&path, &rel)
                .map_err(|e| DayDocError::io(&path, e))?;
            *files += 1;
        }
    }
    Ok(())
}

fn parse_name(name: &str) -> Option<NaiveDate> {
    let rest = name.strip_prefix(PREFIX)?;
    let stem = rest
        .strip_suffix(ENCRYPTED_SUFFIX)
        .or_else(|| rest.strip_suffix(SUFFIX))?;
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()
}

/// Which backups to keep: last 7 days, the newest of each of the last 4 weeks,
/// then the newest of each of the last 6 months.
pub fn keep_flags(dates: &[NaiveDate], today: NaiveDate) -> Vec<bool> {
    let mut flags = vec![false; dates.len()];
    let daily_cutoff = today - Duration::days((DAILY_KEEP as i64) - 1);
    let weekly_cutoff = daily_cutoff - Duration::days((WEEKLY_KEEP as i64) * 7);
    let monthly_cutoff = today - Duration::days(200);

    for (index, date) in dates.iter().enumerate() {
        if *date >= daily_cutoff {
            flags[index] = true;
        }
    }

    // Newest backup per ISO week inside the weekly window.
    let mut weeks: BTreeMap<(i32, u32), (usize, NaiveDate)> = BTreeMap::new();
    for (index, date) in dates.iter().enumerate() {
        if *date >= weekly_cutoff && *date < daily_cutoff {
            let key = (date.iso_week().year(), date.iso_week().week());
            let better = match weeks.get(&key) {
                Some((_, existing)) => *date > *existing,
                None => true,
            };
            if better {
                weeks.insert(key, (index, *date));
            }
        }
    }
    let mut weekly: Vec<((i32, u32), usize)> = weeks.into_iter().map(|(k, (i, _))| (k, i)).collect();
    weekly.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, index) in weekly.into_iter().take(WEEKLY_KEEP) {
        flags[index] = true;
    }

    // Newest backup per calendar month inside the monthly window.
    let mut months: BTreeMap<(i32, u32), (usize, NaiveDate)> = BTreeMap::new();
    for (index, date) in dates.iter().enumerate() {
        if *date >= monthly_cutoff && *date < weekly_cutoff {
            let key = (date.year(), date.month());
            let better = match months.get(&key) {
                Some((_, existing)) => *date > *existing,
                None => true,
            };
            if better {
                months.insert(key, (index, *date));
            }
        }
    }
    let mut monthly: Vec<((i32, u32), usize)> = months.into_iter().map(|(k, (i, _))| (k, i)).collect();
    monthly.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, index) in monthly.into_iter().take(MONTHLY_KEEP) {
        flags[index] = true;
    }

    flags
}

/// Existing backups in a folder, oldest first.
pub fn existing(out_dir: &Path) -> Vec<(PathBuf, NaiveDate)> {
    let mut found: Vec<(PathBuf, NaiveDate)> = Vec::new();
    let entries = match std::fs::read_dir(out_dir) {
        Ok(entries) => entries,
        Err(_) => return found,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(date) = parse_name(&name) {
            found.push((entry.path(), date));
        }
    }
    found.sort_by_key(|entry| entry.1);
    found
}

/// Write today's backup and apply retention. Encrypted backups are written as
/// `.tar.gz.enc` and refuse to be written at all without a key.
pub fn run(vault: &Vault, out_dir: &Path) -> Result<BackupReport> {
    let encrypt = vault.config().backup_encrypt;
    let key = if encrypt {
        let text = secret::resolve(vault, secret::BACKUP).ok_or_else(|| {
            DayDocError::Config(
                "backup_encrypt = true but no backup key is set. Create one with \
                 `daydoc key set backup --generate`, or set DAYDOC_BACKUP_KEY."
                    .to_string(),
            )
        })?;
        Some(crypto::key_from_text(&text)?)
    } else {
        None
    };

    std::fs::create_dir_all(out_dir).map_err(|e| DayDocError::io(out_dir, e))?;

    let today = chrono::Local::now().date_naive();
    let suffix = if encrypt { ENCRYPTED_SUFFIX } else { SUFFIX };
    let name = format!("{PREFIX}{}{suffix}", today.format("%Y-%m-%d"));
    let path = out_dir.join(&name);
    let tmp = out_dir.join(format!(".{}-{}.tmp", name, ids::new_ulid()));

    let mut files = 0usize;
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = Builder::new(encoder);
    add_tree(&mut builder, vault.root(), vault.root(), &mut files)?;
    let encoder = builder.into_inner().map_err(|e| DayDocError::io(&tmp, e))?;
    let archive = encoder.finish().map_err(|e| DayDocError::io(&tmp, e))?;

    let payload = match &key {
        Some(key) => crypto::encrypt(key, &archive)?,
        None => archive,
    };
    std::fs::write(&tmp, &payload).map_err(|e| DayDocError::io(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| DayDocError::io(&path, e))?;
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    let backups = existing(out_dir);
    let dates: Vec<NaiveDate> = backups.iter().map(|(_, date)| *date).collect();
    let flags = keep_flags(&dates, today);
    let mut pruned = Vec::new();
    let mut kept = 0usize;
    for (index, (candidate, _)) in backups.iter().enumerate() {
        if flags.get(index).copied().unwrap_or(true) {
            kept += 1;
        } else if std::fs::remove_file(candidate).is_ok() {
            pruned.push(
                candidate
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            );
        } else {
            kept += 1;
        }
    }

    vault.audit(
        "backup",
        &name,
        &format!(
            "{files} files, {bytes} bytes, {}{} pruned",
            if encrypt { "encrypted, " } else { "" },
            pruned.len()
        ),
    )?;

    Ok(BackupReport {
        path,
        bytes,
        files,
        pruned,
        kept,
        encrypted: encrypt,
    })
}

#[derive(Debug, Clone)]
pub struct RestoreReport {
    pub files: usize,
    pub encrypted: bool,
}

/// Unpack a backup. Entry paths that would escape `dest` are skipped, not
/// followed, and a destination that already holds files is refused unless
/// `force` is set — a restore over a live vault would overwrite newer notes.
pub fn restore(
    file: &Path,
    dest: &Path,
    key: Option<&str>,
    force: bool,
) -> Result<RestoreReport> {
    let payload = std::fs::read(file).map_err(|e| DayDocError::io(file, e))?;
    let (data, encrypted) = if crypto::is_encrypted(&payload) {
        let text = key.ok_or_else(|| {
            DayDocError::Config(
                "this backup is encrypted; set the backup key first with \
                 `daydoc key set backup` or DAYDOC_BACKUP_KEY"
                    .to_string(),
            )
        })?;
        let key = crypto::key_from_text(text)?;
        (crypto::decrypt(&key, &payload)?, true)
    } else {
        (payload, false)
    };

    if !force && dest.is_dir() {
        let occupied = std::fs::read_dir(dest)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if occupied {
            return Err(DayDocError::Config(format!(
                "{} is not empty; pass --force to unpack into it anyway",
                dest.display()
            )));
        }
    }
    std::fs::create_dir_all(dest).map_err(|e| DayDocError::io(dest, e))?;
    let decoder = GzDecoder::new(&data[..]);
    let mut archive = Archive::new(decoder);
    let mut files = 0usize;
    let entries = archive
        .entries()
        .map_err(|e| DayDocError::Config(format!("not a tar.gz archive: {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| DayDocError::Config(format!("bad entry: {e}")))?;
        let unpacked = entry
            .unpack_in(dest)
            .map_err(|e| DayDocError::io(dest, e))?;
        if unpacked {
            files += 1;
        }
    }
    Ok(RestoreReport { files, encrypted })
}

/// Where backups go unless told otherwise.
pub fn default_dir(vault: &Vault) -> PathBuf {
    vault.state_dir().join("backups")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("date")
    }

    #[test]
    fn the_last_seven_days_are_all_kept() {
        let today = date(2026, 11, 19);
        let dates: Vec<NaiveDate> = (0..7).map(|i| today - Duration::days(i)).collect();
        assert!(keep_flags(&dates, today).iter().all(|k| *k));
    }

    #[test]
    fn older_than_a_week_keeps_one_per_week() {
        let today = date(2026, 11, 19);
        // Every day for 40 days: at most 7 daily + 4 weekly survive.
        let dates: Vec<NaiveDate> = (0..40).map(|i| today - Duration::days(i)).collect();
        let flags = keep_flags(&dates, today);
        let kept = flags.iter().filter(|k| **k).count();
        assert!(
            kept <= DAILY_KEEP + WEEKLY_KEEP + MONTHLY_KEEP,
            "kept {kept}"
        );
        assert!(kept >= DAILY_KEEP, "kept {kept}");
    }

    #[test]
    fn very_old_backups_are_dropped() {
        let today = date(2026, 11, 19);
        let dates = vec![date(2020, 1, 1), today];
        let flags = keep_flags(&dates, today);
        assert!(!flags[0]);
        assert!(flags[1]);
    }

    #[test]
    fn backup_writes_a_tarball_and_prunes_old_ones() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("write");
        let dir = vault.state_dir().join("backups");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("daydoc-2020-01-01.tar.gz"), b"old").expect("write old");

        let report = run(&vault, &dir).expect("backup");
        assert!(report.path.exists());
        assert!(report.bytes > 0);
        assert!(report.files >= 2);
        assert!(report.pruned.contains(&"daydoc-2020-01-01.tar.gz".to_string()));
        assert!(!dir.join("daydoc-2020-01-01.tar.gz").exists());
    }

    #[test]
    fn encryption_fails_closed_without_a_key() {
        let mut vault = temp_vault();
        vault.config_mut().backup_encrypt = true;
        let dir = vault.state_dir().join("backups");
        // A fresh temp vault has no key in any store: file, keychain or env.
        assert!(secret::resolve(&vault, secret::BACKUP).is_none());
        let err = run(&vault, &dir).expect_err("no key, no backup");
        assert!(err.to_string().contains("backup key"));
        assert!(!dir.exists() || existing(&dir).is_empty());
    }

    #[test]
    fn an_encrypted_backup_round_trips_through_restore() {
        let vault = temp_vault();
        vault
            .write(
                "inbox/a.md",
                "---\nid: 01A\ntype: idea\ntitle: A\n---\nbody\n",
            )
            .expect("write");
        let key = crypto::new_key_hex().expect("key");
        secret::set(&vault, secret::BACKUP, &key).expect("store key");

        let mut vault = vault;
        vault.config_mut().backup_encrypt = true;
        let dir = vault.state_dir().join("backups");
        let report = run(&vault, &dir).expect("backup");
        assert!(report.encrypted);
        assert!(report.path.to_string_lossy().ends_with(".tar.gz.enc"));
        assert!(report.render().contains("AES-256-GCM"), "{}", report.render());

        let payload = std::fs::read(&report.path).expect("read");
        assert!(crypto::is_encrypted(&payload));
        assert!(!String::from_utf8_lossy(&payload).contains("title: A"));

        let dest = vault.state_dir().join("restored");
        let restored = restore(&report.path, &dest, Some(&key), false).expect("restore");
        assert!(restored.encrypted);
        assert!(restored.files >= 2);
        let text = std::fs::read_to_string(dest.join("inbox/a.md")).expect("restored note");
        assert!(text.contains("title: A"));
    }

    #[test]
    fn restoring_an_encrypted_backup_without_a_key_fails_closed() {
        let vault = temp_vault();
        let key = crypto::new_key_hex().expect("key");
        secret::set(&vault, secret::BACKUP, &key).expect("store key");
        let mut vault = vault;
        vault.config_mut().backup_encrypt = true;
        let dir = vault.state_dir().join("backups");
        let report = run(&vault, &dir).expect("backup");
        let dest = vault.state_dir().join("restored");
        let err = restore(&report.path, &dest, None, false).expect_err("no key");
        assert!(err.to_string().contains("encrypted"));
        assert!(!dest.join("inbox").exists());
    }

    #[test]
    fn restore_refuses_a_folder_that_already_has_files() {
        let vault = temp_vault();
        vault
            .write("inbox/c.md", "---\nid: 01C\ntype: idea\ntitle: C\n---\nbody\n")
            .expect("write");
        let dir = vault.state_dir().join("backups");
        let report = run(&vault, &dir).expect("backup");

        let dest = vault.state_dir().join("occupied");
        std::fs::create_dir_all(&dest).expect("dir");
        std::fs::write(dest.join("newer-note.md"), "mine").expect("write");

        let err = restore(&report.path, &dest, None, false).expect_err("refused");
        assert!(err.to_string().contains("--force"), "{err}");
        assert_eq!(
            std::fs::read_to_string(dest.join("newer-note.md")).expect("read"),
            "mine",
            "the refusal must not touch the folder"
        );

        // With --force the same folder is used.
        let restored = restore(&report.path, &dest, None, true).expect("forced");
        assert!(restored.files >= 1);
    }

    #[test]
    fn a_plaintext_backup_restores_without_a_key() {
        let vault = temp_vault();
        vault
            .write("inbox/b.md", "---\nid: 01B\ntype: idea\ntitle: B\n---\nbody\n")
            .expect("write");
        let dir = vault.state_dir().join("backups");
        let report = run(&vault, &dir).expect("backup");
        assert!(!report.encrypted);
        let dest = vault.state_dir().join("restored");
        let restored = restore(&report.path, &dest, None, false).expect("restore");
        assert!(!restored.encrypted);
        assert!(dest.join("inbox/b.md").exists());
    }
}
