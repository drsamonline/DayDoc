//! Watching the vault while an external editor writes to it.
//!
//! VS Code, Obsidian and a plain `vim` all edit the same files. The watcher
//! reindexes on change and, on a timer, commits — the debounce keeps a single
//! editor save from becoming twenty index passes, and a burst of saves from
//! becoming twenty commits.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::{Event, RecursiveMode, Watcher};

use crate::error::{DayDocError, Result};
use crate::vault::Vault;

pub const DEFAULT_DEBOUNCE_MS: u64 = 500;

#[derive(Debug, Clone)]
pub struct WatchConfig {
    pub debounce: Duration,
    pub auto_commit: bool,
    pub commit_interval: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            debounce: Duration::from_millis(DEFAULT_DEBOUNCE_MS),
            auto_commit: true,
            commit_interval: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BatchOutcome {
    pub changed: Vec<String>,
    pub reindexed: usize,
    pub committed: bool,
    pub commit_head: Option<String>,
}

/// Paths worth reacting to: notes, not our own bookkeeping, not the git
/// directory (a commit would otherwise trigger the next commit), not temp files.
pub fn is_interesting(rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }
    let lowered = rel.to_ascii_lowercase();
    if lowered.starts_with("_meta/")
        || lowered.starts_with("_trash/")
        || lowered.starts_with(".git/")
        || lowered.starts_with("assets/")
    {
        return false;
    }
    if rel.split('/').any(|part| part.starts_with('.')) {
        return false;
    }
    lowered.ends_with(".md") || lowered.ends_with(".toml")
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn absorb(root: &Path, event: &Event, into: &mut BTreeSet<String>) {
    for path in &event.paths {
        if let Some(rel) = relative(root, path) {
            if is_interesting(&rel) {
                into.insert(rel);
            }
        }
    }
}

/// Wait for one batch of changes: block for the first event (or `first_wait`),
/// then keep collecting until the vault has been quiet for `idle`.
pub fn drain_batch(
    root: &Path,
    rx: &Receiver<notify::Result<Event>>,
    idle: Duration,
    first_wait: Option<Duration>,
) -> Result<Vec<String>> {
    let mut seen: BTreeSet<String> = BTreeSet::new();

    let first = match first_wait {
        Some(limit) => match rx.recv_timeout(limit) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => return Ok(Vec::new()),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(DayDocError::Watch("the watcher stopped".to_string()))
            }
        },
        None => rx
            .recv()
            .map_err(|_| DayDocError::Watch("the watcher stopped".to_string()))?,
    };
    match first {
        Ok(event) => absorb(root, &event, &mut seen),
        Err(err) => return Err(DayDocError::Watch(err.to_string())),
    }

    loop {
        match rx.recv_timeout(idle) {
            Ok(Ok(event)) => absorb(root, &event, &mut seen),
            Ok(Err(err)) => return Err(DayDocError::Watch(err.to_string())),
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(seen.into_iter().collect())
}

/// Index the batch's paths and, when asked, commit. A failed commit does not
/// stop the watcher: the vault is already on disk.
///
/// Only the changed paths are touched. A full rebuild per editor save would make
/// the watcher cost grow with the vault rather than with the edit.
pub fn handle_batch(vault: &Vault, changed: &[String], commit_now: bool) -> Result<BatchOutcome> {
    let mut indexed = 0usize;
    for rel in changed {
        if !is_interesting(rel) {
            continue;
        }
        if crate::index::upsert_path(vault, rel)? {
            indexed += 1;
        }
    }
    crate::index::stamp(vault)?;

    let mut outcome = BatchOutcome {
        changed: changed.to_vec(),
        reindexed: indexed,
        committed: false,
        commit_head: None,
    };

    if commit_now && vault.config().auto_commit {
        #[cfg(feature = "git")]
        {
            match crate::sync::commit(vault, Some("daydoc: autosave")) {
                Ok(commit) => {
                    outcome.committed = commit.committed;
                    outcome.commit_head = commit.head;
                }
                Err(err) => eprintln!("daydoc: commit skipped: {err}"),
            }
        }
        #[cfg(not(feature = "git"))]
        {
            let _ = commit_now;
        }
    }
    Ok(outcome)
}

/// Watch until the process is stopped.
pub fn run(vault: &Vault, config: &WatchConfig) -> Result<()> {
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |result| {
        let _ = tx.send(result);
    })
    .map_err(|err| DayDocError::Watch(err.to_string()))?;
    watcher
        .watch(vault.root(), RecursiveMode::Recursive)
        .map_err(|err| DayDocError::Watch(err.to_string()))?;

    println!(
        "watching {} — debounce {} ms, commit every {} s",
        vault.root().display(),
        config.debounce.as_millis(),
        config.commit_interval.as_secs()
    );

    let mut last_commit = Instant::now();
    loop {
        let changed = drain_batch(vault.root(), &rx, config.debounce, None)?;
        if changed.is_empty() {
            continue;
        }
        let commit_now =
            config.auto_commit && last_commit.elapsed() >= config.commit_interval;
        let outcome = handle_batch(vault, &changed, commit_now)?;
        println!(
            "{} change(s) → {} indexed{}",
            outcome.changed.len(),
            outcome.reindexed,
            if outcome.committed { ", committed" } else { "" }
        );
        if outcome.committed {
            last_commit = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::test_support::temp_vault;

    #[test]
    fn bookkeeping_is_not_a_change() {
        assert!(is_interesting("inbox/a.md"));
        assert!(is_interesting("decisions/DR-0001.md"));
        assert!(!is_interesting("_meta/audit.jsonl"));
        assert!(!is_interesting("_trash/old.md"));
        assert!(!is_interesting(".git/index"));
        assert!(!is_interesting(".daydoc-tmp-01J.md"));
        assert!(!is_interesting("assets/ab/cd.png"));
        assert!(!is_interesting(""));
    }

    #[test]
    fn a_batch_reindexes_the_vault() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01A\ntype: idea\ntitle: A\n---\nbody\n")
            .expect("write");
        let outcome = handle_batch(&vault, &["inbox/a.md".to_string()], false).expect("handle");
        assert_eq!(outcome.reindexed, 1);
        assert!(!outcome.committed);
    }

    #[test]
    fn a_deleted_note_leaves_the_index() {
        let vault = temp_vault();
        vault
            .write("inbox/a.md", "---\nid: 01A\ntype: idea\ntitle: Gone soon\n---\nbody\n")
            .expect("write");
        handle_batch(&vault, &["inbox/a.md".to_string()], false).expect("index it");
        assert!(crate::index::candidates(&vault, &["gone".to_string()]).is_some());

        std::fs::remove_file(vault.abs("inbox/a.md").expect("abs")).expect("delete");
        let outcome = handle_batch(&vault, &["inbox/a.md".to_string()], false).expect("handle");
        assert_eq!(outcome.reindexed, 0);
        let hits = crate::index::candidates(&vault, &["gone".to_string()]).expect("indexed");
        assert!(hits.is_empty(), "a deleted note is still in the index");
    }

    #[test]
    fn a_write_is_seen_and_debounced() {
        let vault = temp_vault();
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |result| {
            let _ = tx.send(result);
        })
        .expect("watcher");
        watcher
            .watch(vault.root(), RecursiveMode::Recursive)
            .expect("watch");

        // Let the platform watcher finish registering before writing.
        std::thread::sleep(Duration::from_millis(400));
        vault
            .write(
                "inbox/live.md",
                "---\nid: 01L\ntype: idea\ntitle: Live\n---\nbody\n",
            )
            .expect("write");

        let changed = drain_batch(
            vault.root(),
            &rx,
            Duration::from_millis(300),
            Some(Duration::from_secs(15)),
        )
        .expect("batch");
        assert!(
            changed.iter().any(|rel| rel.contains("inbox/live.md")),
            "did not see the write: {changed:?}"
        );
    }

    #[test]
    fn an_idle_watcher_returns_nothing() {
        let vault = temp_vault();
        let (_tx, rx) = channel::<notify::Result<Event>>();
        let changed = drain_batch(
            vault.root(),
            &rx,
            Duration::from_millis(50),
            Some(Duration::from_millis(50)),
        )
        .expect("batch");
        assert!(changed.is_empty());
    }
}
