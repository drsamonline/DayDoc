//! DayDoc — decision-first office notes.
//!
//! A note is evidence. A decision is the record. This crate is the vault
//! engine: plain Markdown plus YAML on disk, no account, no server, no
//! telemetry, fully offline.
//!
//! The Tier 0 surface (capture, classification, decision ledger, supersession,
//! meeting-to-decision extraction, SOP proposals, stale detection, search,
//! handover, digests) is implemented without any model or network call.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod attach;
pub mod backup;
pub mod capture;
pub mod classify;
pub mod cluster;
pub mod config;
pub mod crypto;
pub mod daily;
pub mod error;
pub mod export;
pub mod frontmatter;
pub mod handover;
pub mod ids;
pub mod index;
pub mod ledger;
pub mod lint;
pub mod llm;
pub mod meeting;
pub mod migrate;
pub mod note;
pub mod search;
pub mod secret;
pub mod sop;
pub mod stale;
pub mod stats;
#[cfg(feature = "git")]
pub mod sync;
pub mod summarize;
pub mod types;
pub mod vault;
pub mod watch;

pub use error::{DayDocError, Result};
pub use vault::Vault;

/// Crate version, surfaced by `daydoc --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
