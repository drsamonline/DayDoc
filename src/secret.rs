//! Where keys live.
//!
//! Two stores, tried in order: the environment (`DAYDOC_<NAME>_KEY`, for CI and
//! scripted runs) and a `0600` file in the state directory. The OS keychain is
//! the next store behind the same trait — the interface exists so that swapping
//! it in touches this file only.
//!
//! Threat model, stated plainly: the file store keeps a key out of the vault, out
//! of `daydoc.toml`, out of git and out of logs, and readable only by the account
//! that owns it. It does not defend against a compromised account. When the
//! keychain lands, that sentence gets shorter.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{DayDocError, Result};
use crate::vault::Vault;

pub const LLM: &str = "llm";
pub const BACKUP: &str = "backup";

pub trait KeyStore {
    fn get(&self, name: &str) -> Option<String>;
    fn set(&self, name: &str, value: &str) -> Result<()>;
    fn clear(&self, name: &str) -> Result<bool>;
}

fn env_name(name: &str) -> String {
    format!("DAYDOC_{}_KEY", name.to_ascii_uppercase().replace('-', "_"))
}

/// Keys supplied by the environment. Nothing is written.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvStore;

impl KeyStore for EnvStore {
    fn get(&self, name: &str) -> Option<String> {
        let value = std::env::var(env_name(name)).ok()?;
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    fn set(&self, _name: &str, _value: &str) -> Result<()> {
        Err(DayDocError::Config(
            "keys cannot be set through the environment; export the variable instead".to_string(),
        ))
    }

    fn clear(&self, _name: &str) -> Result<bool> {
        Ok(false)
    }
}

/// Keys in `<state>/keys/<name>`, mode 0600 on unix.
#[derive(Debug, Clone)]
pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn new(dir: impl Into<PathBuf>) -> FileStore {
        FileStore { dir: dir.into() }
    }

    pub fn for_vault(vault: &Vault) -> FileStore {
        FileStore::new(vault.state_dir().join("keys"))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl KeyStore for FileStore {
    fn get(&self, name: &str) -> Option<String> {
        let raw = std::fs::read_to_string(self.path(name)).ok()?;
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            return Err(DayDocError::Config("refusing to store an empty key".to_string()));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| DayDocError::io(&self.dir, e))?;
        let path = self.path(name);
        std::fs::write(&path, value).map_err(|e| DayDocError::io(&path, e))?;
        restrict(&path)?;
        Ok(())
    }

    fn clear(&self, name: &str) -> Result<bool> {
        let path = self.path(name);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(DayDocError::io(&path, err)),
        }
    }
}

fn restrict(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, permissions).map_err(|e| DayDocError::io(path, e))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// In-memory store, for tests and for a single run.
#[derive(Debug, Default)]
pub struct MemoryStore {
    values: std::sync::Mutex<BTreeMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }
}

impl KeyStore for MemoryStore {
    fn get(&self, name: &str) -> Option<String> {
        self.values.lock().ok()?.get(name).cloned()
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        if let Ok(mut values) = self.values.lock() {
            values.insert(name.to_string(), value.trim().to_string());
        }
        Ok(())
    }

    fn clear(&self, name: &str) -> Result<bool> {
        if let Ok(mut values) = self.values.lock() {
            return Ok(values.remove(name).is_some());
        }
        Ok(false)
    }
}

/// The key for `name`, environment first, then the state directory.
pub fn resolve(vault: &Vault, name: &str) -> Option<String> {
    EnvStore
        .get(name)
        .or_else(|| FileStore::for_vault(vault).get(name))
}

pub fn set(vault: &Vault, name: &str, value: &str) -> Result<PathBuf> {
    FileStore::for_vault(vault).set(name, value)?;
    Ok(FileStore::for_vault(vault).path(name))
}

pub fn clear(vault: &Vault, name: &str) -> Result<bool> {
    FileStore::for_vault(vault).clear(name)
}

pub fn is_set(vault: &Vault, name: &str) -> bool {
    resolve(vault, name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn a_key_round_trips_through_the_file_store() {
        let vault = temp_vault();
        assert!(resolve(&vault, BACKUP).is_none());
        set(&vault, BACKUP, "  abc123  ").expect("set");
        assert_eq!(resolve(&vault, BACKUP).as_deref(), Some("abc123"));
        assert!(is_set(&vault, BACKUP));
        assert!(clear(&vault, BACKUP).expect("clear"));
        assert!(resolve(&vault, BACKUP).is_none());
        assert!(!clear(&vault, BACKUP).expect("clear again"));
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let vault = temp_vault();
        let path = set(&vault, LLM, "sk-test").expect("set");
        let mode = std::fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
    }

    #[test]
    fn an_empty_key_is_refused() {
        let vault = temp_vault();
        assert!(set(&vault, BACKUP, "   ").is_err());
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        let vault = temp_vault();
        set(&vault, "probe", "from-file").expect("set");
        std::env::set_var("DAYDOC_PROBE_KEY", "from-env");
        assert_eq!(resolve(&vault, "probe").as_deref(), Some("from-env"));
        std::env::remove_var("DAYDOC_PROBE_KEY");
        assert_eq!(resolve(&vault, "probe").as_deref(), Some("from-file"));
    }

    #[test]
    fn the_memory_store_is_self_contained() {
        let store = MemoryStore::new();
        assert!(store.get(BACKUP).is_none());
        store.set(BACKUP, "k").expect("set");
        assert_eq!(store.get(BACKUP).as_deref(), Some("k"));
        assert!(store.clear(BACKUP).expect("clear"));
    }
}
