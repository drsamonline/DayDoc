//! `daydoc.toml`. Small on purpose: every knob here changes behaviour the user
//! can observe, and nothing here phones home.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{DayDocError, Result};

pub const CONFIG_FILE: &str = "daydoc.toml";

/// Per-type staleness thresholds in days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StaleThresholds {
    pub sop: i64,
    pub reference: i64,
    #[serde(rename = "project-update")]
    pub project_update: i64,
    pub meeting: i64,
    #[serde(rename = "decision-proposed")]
    pub decision_proposed: i64,
}

impl Default for StaleThresholds {
    fn default() -> Self {
        StaleThresholds {
            sop: 180,
            reference: 365,
            project_update: 30,
            meeting: 365,
            decision_proposed: 90,
        }
    }
}

impl StaleThresholds {
    pub fn for_key(&self, key: &str) -> Option<i64> {
        match key {
            "sop" => Some(self.sop),
            "reference" => Some(self.reference),
            "project-update" => Some(self.project_update),
            "meeting" => Some(self.meeting),
            "decision-proposed" => Some(self.decision_proposed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub vault_root: String,
    pub read_only_vault: bool,
    pub llm_mode: String,
    pub llm_provider: String,
    pub llm_send_content: bool,
    pub summarizer: String,
    pub min_classification_confidence: f32,
    /// Where `model.onnx` and `vocab.txt` live. Empty means `<state>/models`.
    pub embed_model_dir: String,
    pub auto_commit: bool,
    pub auto_commit_debounce_secs: u64,
    pub weekly_digest: bool,
    pub trash_retention_days: i64,
    pub backup_enabled: bool,
    pub backup_encrypt: bool,
    pub stale_thresholds: StaleThresholds,
    pub mobile_enabled: bool,
    pub mobile_capture_only: bool,
    pub calendar_enabled: bool,
    pub telemetry: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            vault_root: String::new(),
            read_only_vault: false,
            llm_mode: "off".to_string(),
            llm_provider: String::new(),
            llm_send_content: false,
            summarizer: "extractive".to_string(),
            min_classification_confidence: 0.7,
            embed_model_dir: String::new(),
            auto_commit: true,
            auto_commit_debounce_secs: 300,
            weekly_digest: true,
            trash_retention_days: 30,
            backup_enabled: true,
            backup_encrypt: false,
            stale_thresholds: StaleThresholds::default(),
            mobile_enabled: false,
            mobile_capture_only: true,
            calendar_enabled: false,
            telemetry: false,
        }
    }
}

impl Config {
    /// Read `daydoc.toml` from a vault root. A missing file is not an error:
    /// DayDoc runs on defaults rather than refusing to open a folder.
    pub fn load_from_vault(root: &Path) -> Result<Config> {
        let path = root.join(CONFIG_FILE);
        if !path.exists() {
            let cfg = Config {
                vault_root: root.display().to_string(),
                ..Config::default()
            };
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| DayDocError::io(&path, e))?;
        let mut cfg: Config =
            toml::from_str(&raw).map_err(|e| DayDocError::Config(format!("{}: {e}", path.display())))?;
        if cfg.vault_root.is_empty() {
            cfg.vault_root = root.display().to_string();
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn load_file(path: &Path) -> Result<Config> {
        let raw = std::fs::read_to_string(path).map_err(|e| DayDocError::io(path, e))?;
        let cfg: Config = toml::from_str(&raw)
            .map_err(|e| DayDocError::Config(format!("{}: {e}", path.display())))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        let mode = self.llm_mode.as_str();
        if !matches!(mode, "off" | "byok" | "auto") {
            return Err(DayDocError::Config(format!(
                "llm_mode must be \"off\", \"byok\" or \"auto\", found \"{mode}\""
            )));
        }
        if self.telemetry {
            // The schema carries the key so an old config never fails to parse,
            // but nothing in DayDoc can honour it.
            return Err(DayDocError::Config(
                "telemetry is not implemented and cannot be enabled; set telemetry = false"
                    .to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.min_classification_confidence) {
            return Err(DayDocError::Config(
                "min_classification_confidence must be between 0.0 and 1.0".to_string(),
            ));
        }
        if self.trash_retention_days < 1 {
            return Err(DayDocError::Config(
                "trash_retention_days must be at least 1; 0 would delete the trash \
                 as soon as it is written"
                    .to_string(),
            ));
        }
        if self.backup_encrypt && !self.backup_enabled {
            return Err(DayDocError::Config(
                "backup_encrypt requires backup_enabled".to_string(),
            ));
        }
        Ok(())
    }

    pub fn to_toml(&self) -> String {
        let mut cfg = self.clone();
        cfg.vault_root = self.vault_root.clone();
        toml::to_string_pretty(&cfg).unwrap_or_else(|_| String::from("# daydoc config\n"))
    }

    pub fn write_to(&self, root: &Path) -> Result<PathBuf> {
        let path = root.join(CONFIG_FILE);
        std::fs::write(&path, self.to_toml()).map_err(|e| DayDocError::io(&path, e))?;
        Ok(path)
    }

    /// Keys `daydoc config set` understands.
    pub const SETTABLE: [&'static str; 18] = [
        "llm_mode",
        "llm_provider",
        "llm_send_content",
        "min_classification_confidence",
        "embed_model_dir",
        "auto_commit",
        "auto_commit_debounce_secs",
        "weekly_digest",
        "trash_retention_days",
        "backup_enabled",
        "backup_encrypt",
        "read_only_vault",
        "mobile_enabled",
        "calendar_enabled",
        "stale.sop",
        "stale.reference",
        "stale.project-update",
        "stale.decision-proposed",
    ];

    /// Set one key from its textual form. Returns how the value is now stored.
    pub fn set_key(&mut self, key: &str, raw: &str) -> Result<String> {
        let value = raw.trim();
        let boolean = |text: &str| -> Result<bool> {
            match text.to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" | "on" => Ok(true),
                "false" | "no" | "0" | "off" => Ok(false),
                _ => Err(DayDocError::Config(format!("\"{text}\" is not a boolean"))),
            }
        };
        let integer = |text: &str| -> Result<i64> {
            text.parse::<i64>()
                .map_err(|_| DayDocError::Config(format!("\"{text}\" is not a whole number")))
        };

        match key.trim() {
            "llm_mode" => {
                let mode = value.to_ascii_lowercase();
                if !matches!(mode.as_str(), "off" | "byok" | "auto") {
                    return Err(DayDocError::Config(format!(
                        "llm_mode must be \"off\", \"byok\" or \"auto\", not \"{value}\""
                    )));
                }
                self.llm_mode = mode.clone();
                Ok(mode)
            }
            "llm_provider" => {
                self.llm_provider = value.to_string();
                Ok(self.llm_provider.clone())
            }
            "llm_send_content" => {
                self.llm_send_content = boolean(value)?;
                Ok(self.llm_send_content.to_string())
            }
            "embed_model_dir" => {
                self.embed_model_dir = value.to_string();
                Ok(self.embed_model_dir.clone())
            }
            "min_classification_confidence" => {
                self.min_classification_confidence = value.parse::<f32>().map_err(|_| {
                    DayDocError::Config(format!("\"{value}\" is not a confidence between 0 and 1"))
                })?;
                Ok(self.min_classification_confidence.to_string())
            }
            "auto_commit" => {
                self.auto_commit = boolean(value)?;
                Ok(self.auto_commit.to_string())
            }
            "auto_commit_debounce_secs" => {
                self.auto_commit_debounce_secs = integer(value)?.max(0) as u64;
                Ok(self.auto_commit_debounce_secs.to_string())
            }
            "weekly_digest" => {
                self.weekly_digest = boolean(value)?;
                Ok(self.weekly_digest.to_string())
            }
            "trash_retention_days" => {
                self.trash_retention_days = integer(value)?.max(1);
                Ok(self.trash_retention_days.to_string())
            }
            "backup_enabled" => {
                self.backup_enabled = boolean(value)?;
                Ok(self.backup_enabled.to_string())
            }
            "backup_encrypt" => {
                self.backup_encrypt = boolean(value)?;
                Ok(self.backup_encrypt.to_string())
            }
            "read_only_vault" => {
                self.read_only_vault = boolean(value)?;
                Ok(self.read_only_vault.to_string())
            }
            "mobile_enabled" => {
                self.mobile_enabled = boolean(value)?;
                Ok(self.mobile_enabled.to_string())
            }
            "calendar_enabled" => {
                self.calendar_enabled = boolean(value)?;
                Ok(self.calendar_enabled.to_string())
            }
            "stale.sop" => {
                self.stale_thresholds.sop = integer(value)?.max(1);
                Ok(self.stale_thresholds.sop.to_string())
            }
            "stale.reference" => {
                self.stale_thresholds.reference = integer(value)?.max(1);
                Ok(self.stale_thresholds.reference.to_string())
            }
            "stale.project-update" => {
                self.stale_thresholds.project_update = integer(value)?.max(1);
                Ok(self.stale_thresholds.project_update.to_string())
            }
            "stale.decision-proposed" => {
                self.stale_thresholds.decision_proposed = integer(value)?.max(1);
                Ok(self.stale_thresholds.decision_proposed.to_string())
            }
            other => Err(DayDocError::Config(format!(
                "unknown key \"{other}\"; try one of: {}",
                Config::SETTABLE.join(", ")
            ))),
        }
    }

    pub fn llm_enabled(&self) -> bool {
        self.llm_mode != "off"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_by_default() {
        let c = Config::default();
        assert_eq!(c.llm_mode, "off");
        assert!(!c.telemetry);
        assert!(!c.mobile_enabled);
        assert!(!c.calendar_enabled);
        assert_eq!(c.stale_thresholds.sop, 180);
        c.validate().expect("defaults validate");
    }

    #[test]
    fn stale_keys_use_hyphenated_names() {
        let c = Config::default();
        assert_eq!(c.stale_thresholds.for_key("project-update"), Some(30));
        assert_eq!(c.stale_thresholds.for_key("decision-proposed"), Some(90));
        assert_eq!(c.stale_thresholds.for_key("unknown"), None);
    }

    #[test]
    fn toml_round_trip_keeps_hyphenated_keys() {
        let c = Config::default();
        let text = c.to_toml();
        assert!(text.contains("project-update"));
        let parsed: Config = toml::from_str(&text).expect("round trip");
        assert_eq!(parsed.stale_thresholds, c.stale_thresholds);
        assert_eq!(parsed.llm_mode, "off");
    }

    #[test]
    fn set_key_round_trips_and_rejects_nonsense() {
        let mut c = Config::default();
        assert_eq!(c.set_key("stale.sop", "30").expect("set"), "30");
        assert_eq!(c.stale_thresholds.sop, 30);
        assert_eq!(c.set_key("llm_mode", "byok").expect("set"), "byok");
        c.validate().expect("byok validates");
        assert!(c.set_key("llm_mode", "cloud").is_err());
        assert!(c.set_key("weekly_digest", "maybe").is_err());
        assert!(c.set_key("nonsense", "1").is_err());
        assert!(c.set_key("telemetry", "true").is_err());
    }

    #[test]
    fn the_trash_retention_has_a_floor() {
        let mut c = Config::default();
        assert_eq!(c.set_key("trash_retention_days", "0").expect("set"), "1");
        c.trash_retention_days = 0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn telemetry_cannot_be_enabled() {
        let mut c = Config::default();
        c.telemetry = true;
        assert!(c.validate().is_err());
    }

    #[test]
    fn bad_llm_mode_is_rejected() {
        let mut c = Config::default();
        c.llm_mode = "cloud".into();
        assert!(c.validate().is_err());
    }
}
