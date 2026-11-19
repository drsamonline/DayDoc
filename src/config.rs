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
            let mut cfg = Config::default();
            cfg.vault_root = root.display().to_string();
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
        if self.mobile_enabled && !self.mobile_capture_only {
            return Err(DayDocError::Config(
                "mobile reading is not implemented at v1; keep mobile_capture_only = true"
                    .to_string(),
            ));
        }
        if self.backup_encrypt && self.backup_enabled == false {
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
