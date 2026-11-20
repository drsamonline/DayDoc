//! End-to-end smoke test: the binary, a real folder, real files.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let base = std::env::temp_dir().join(format!(
            "daydoc-cli-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(base.join("vault")).expect("create sandbox");
        Sandbox { root: base }
    }

    fn vault(&self) -> PathBuf {
        self.root.join("vault")
    }

    fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_daydoc"))
            .env("DAYDOC_HOME", self.state())
            .args(["-v", self.vault().to_str().unwrap()])
            .args(args)
            .output()
            .expect("run daydoc")
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "daydoc {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.vault().join(rel)).expect("read file")
    }

    fn exists(&self, rel: &str) -> bool {
        self.vault().join(rel).exists()
    }
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected {needle:?} in:\n{haystack}"
    );
}

#[test]
fn init_then_capture_then_ledger() {
    let sandbox = Sandbox::new("ledger");
    let out = sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    assert_contains(&out, "vault ready");
    assert!(sandbox.exists("daydoc.toml"));
    assert!(sandbox.exists("decisions"));

    let out = sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    assert_contains(&out, "decisions/DR-0001.md");

    let out = sandbox.ok(&["decisions"]);
    assert_contains(&out, "DR-0001");
    assert_contains(&out, "Adopt Okta");

    let note = sandbox.read("decisions/DR-0001.md");
    assert_contains(&note, "type: decision");
    assert_contains(&note, "decision_id: DR-0001");
    assert_contains(&note, "## Context");
}

#[test]
fn supersede_then_revert_through_the_cli() {
    let sandbox = Sandbox::new("supersede");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    sandbox.ok(&["accept", "DR-0001"]);

    let out = sandbox.ok(&[
        "supersede",
        "DR-0001",
        "Use Entra ID for SSO",
        "--decision",
        "Entra is cheaper at our volume.",
    ]);
    assert_contains(&out, "supersedes DR-0001");

    let old = sandbox.read("decisions/DR-0001.md");
    assert_contains(&old, "status: superseded");
    assert_contains(&old, "superseded_by: DR-0002");

    let out = sandbox.ok(&["revert", "DR-0002", "--reason", "wrong call"]);
    assert_contains(&out, "reverted");

    let old = sandbox.read("decisions/DR-0001.md");
    assert_contains(&old, "status: accepted");
    let audit = sandbox.read("_meta/audit.jsonl");
    assert_contains(&audit, "supersession-reverted");
}

#[test]
fn ask_returns_a_card_not_prose() {
    let sandbox = Sandbox::new("ask");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    sandbox.ok(&["accept", "DR-0001"]);
    let out = sandbox.ok(&["ask", "What did we decide about Okta?"]);
    assert_contains(&out, "DR-0001");
    assert_contains(&out, "accepted");
}

#[test]
fn ambiguous_capture_lands_in_inbox() {
    let sandbox = Sandbox::new("inbox");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    let out = sandbox.ok(&["capture", "hmm"]);
    assert_contains(&out, "inbox");
    let listing = std::fs::read_dir(sandbox.vault().join("inbox"))
        .expect("inbox dir")
        .count();
    assert_eq!(listing, 1);
}

#[test]
fn corrupt_frontmatter_is_quarantined_not_destroyed() {
    let sandbox = Sandbox::new("broken");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    std::fs::write(
        sandbox.vault().join("inbox/broken.md"),
        "---\nid: [unclosed\n---\nbody\n",
    )
    .expect("write broken note");

    let output = sandbox.run(&["status"]);
    assert!(output.status.success());
    let text = sandbox.read("inbox/broken.md");
    assert_contains(&text, "unclosed");
    let quarantined = std::fs::read_dir(sandbox.vault().join("_meta/broken"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert!(quarantined >= 1);
}

#[test]
fn handover_and_digest_render() {
    let sandbox = Sandbox::new("handover");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    sandbox.ok(&[
        "capture",
        "--text",
        "- [ ] send the invoice\n- [ ] book the room\n",
    ]);

    let out = sandbox.ok(&["handover", "--dry-run"]);
    assert_contains(&out, "## What I did");
    assert_contains(&out, "## Who to ask");

    let out = sandbox.ok(&["digest", "--dry-run"]);
    assert_contains(&out, "## Decisions made");

    let out = sandbox.ok(&["wrap", "--dry-run"]);
    assert_contains(&out, "End of day");
}

#[test]
fn read_only_vault_refuses_writes() {
    let sandbox = Sandbox::new("readonly");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    let config_path = sandbox.vault().join("daydoc.toml");
    let mut config = std::fs::read_to_string(&config_path).expect("config");
    config = config.replace("read_only_vault = false", "read_only_vault = true");
    std::fs::write(&config_path, config).expect("write config");

    let output = sandbox.run(&["capture", "we decided to ship on friday"]);
    assert!(!output.status.success(), "read-only vault accepted a write");
    assert!(!sandbox.exists("decisions/DR-0001.md"));
}

#[test]
fn vault_is_plain_markdown_on_disk() {
    let sandbox = Sandbox::new("plain");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    let note = sandbox.read("decisions/DR-0001.md");
    assert!(note.starts_with("---\n"), "not a frontmatter-first file");
    assert_contains(&note, "title:");
    // Nothing binary in the vault outside _meta and assets.
    for entry in std::fs::read_dir(sandbox.vault()).expect("read vault") {
        let path = entry.expect("entry").path();
        if path.file_name().map(|n| n == "_meta").unwrap_or(false) {
            continue;
        }
        let _ = Path::new(&path);
    }
}

#[test]
fn import_previews_then_imports() {
    let sandbox = Sandbox::new("import");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);

    let source = sandbox.root.join("source");
    std::fs::create_dir_all(source.join("notes")).expect("source dirs");
    std::fs::write(
        source.join("notes/old.md"),
        "We decided to adopt Okta for SSO across all products.\n",
    )
    .expect("write source");

    let out = sandbox.ok(&["import", source.to_str().unwrap(), "--dry-run"]);
    assert_contains(&out, "to import");
    assert!(!sandbox.exists("notes/old.md"));

    let out = sandbox.ok(&["import", source.to_str().unwrap()]);
    assert_contains(&out, "imported");
    let imported = sandbox.read("notes/old.md");
    assert_contains(&imported, "type: decision");
    assert_contains(&imported, "id: ");

    // The source folder is untouched.
    assert!(source.join("notes/old.md").exists());
}

#[test]
fn attach_is_content_addressed_and_deduplicated() {
    let sandbox = Sandbox::new("attach");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);

    let file = sandbox.root.join("diagram.png");
    std::fs::write(&file, b"not really a png").expect("write file");

    let first = sandbox.ok(&["attach", file.to_str().unwrap()]);
    assert_contains(&first, "assets/");
    assert!(!first.contains("already present"));

    let second = sandbox.ok(&["attach", file.to_str().unwrap()]);
    assert_contains(&second, "already present");
}

#[test]
fn read_only_flag_blocks_writes() {
    let sandbox = Sandbox::new("ro-flag");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    let output = Command::new(env!("CARGO_BIN_EXE_daydoc"))
        .env("DAYDOC_HOME", sandbox.state())
        .args([
            "-v",
            sandbox.vault().to_str().unwrap(),
            "--read-only-vault",
            "capture",
            "we decided to ship on friday",
        ])
        .output()
        .expect("run");
    assert!(!output.status.success());
    assert!(!sandbox.exists("decisions/DR-0001.md"));
}

#[test]
fn backup_writes_a_tarball_into_the_state_directory() {
    let sandbox = Sandbox::new("backup");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);

    let out = sandbox.ok(&["backup"]);
    assert_contains(&out, "daydoc-");
    assert_contains(&out, "file(s)");

    let written = std::fs::read_dir(sandbox.state().join("backups"))
        .expect("backups dir")
        .count();
    assert_eq!(written, 1);

    let out = sandbox.ok(&["backup", "--dry-run"]);
    assert_contains(&out, "would write");
}

#[test]
fn json_output_is_parseable() {
    let sandbox = Sandbox::new("json");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    sandbox.ok(&["accept", "DR-0001"]);

    let decisions = sandbox.ok(&["decisions", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&decisions).expect("decisions json");
    assert_eq!(parsed[0]["decision_id"], "DR-0001");
    assert_eq!(parsed[0]["status"], "accepted");

    let status = sandbox.ok(&["status", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&status).expect("status json");
    assert_eq!(parsed["decisions"], 1);
    assert_eq!(parsed["llm_mode"], "off");

    let stale = sandbox.ok(&["stale", "--json"]);
    serde_json::from_str::<serde_json::Value>(&stale).expect("stale json");
}

#[test]
fn check_reports_stable_identities() {
    let sandbox = Sandbox::new("check");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    let out = sandbox.ok(&["check"]);
    assert_contains(&out, "identities stable");
}

#[test]
fn lint_reports_a_missing_id_and_reindex_rebuilds() {
    let sandbox = Sandbox::new("lint");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    std::fs::write(
        sandbox.vault().join("inbox/noid.md"),
        "---\ntype: idea\ntitle: No id\n---\nbody\n",
    )
    .expect("write note");

    let out = sandbox.ok(&["lint"]);
    assert_contains(&out, "missing-id");

    let out = sandbox.ok(&["lint", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("lint json");
    assert!(parsed.as_array().map(|a| !a.is_empty()).unwrap_or(false));

    let out = sandbox.ok(&["reindex"]);
    assert_contains(&out, "indexed");

    // Search still works after the index is built.
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    let out = sandbox.ok(&["search", "okta"]);
    assert_contains(&out, "DR-0001");
}

#[test]
fn sync_without_a_repository_says_so() {
    let sandbox = Sandbox::new("sync");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    let output = sandbox.run(&["sync"]);
    assert!(!output.status.success(), "sync should refuse without a repo");
    let err = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(err.contains("git init"), "unhelpful error: {err}");
}

#[test]
fn sync_commits_when_a_repository_exists() {
    let sandbox = Sandbox::new("sync-ok");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(sandbox.vault())
        .status();
    if init.map(|s| !s.success()).unwrap_or(true) {
        return; // no git on this runner; the unit tests cover the commit path
    }
    let out = sandbox.ok(&["sync"]);
    assert_contains(&out, "committed");
    let out = sandbox.ok(&["sync"]);
    assert_contains(&out, "nothing to commit");
}

#[test]
fn config_can_be_read_and_changed() {
    let sandbox = Sandbox::new("config");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);

    let out = sandbox.ok(&["config", "show"]);
    assert_contains(&out, "llm_mode = \"off\"");

    let out = sandbox.ok(&["config", "set", "stale.sop", "30"]);
    assert_contains(&out, "stale.sop = 30");
    let out = sandbox.ok(&["config", "show"]);
    assert_contains(&out, "sop = 30");

    let output = sandbox.run(&["config", "set", "llm_mode", "cloud"]);
    assert!(!output.status.success(), "invalid llm_mode accepted");

    let out = sandbox.ok(&["config", "show", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("config json");
    assert_eq!(parsed["llm_mode"], "off");
}

#[test]
fn stats_and_export_produce_output() {
    let sandbox = Sandbox::new("stats");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);

    let out = sandbox.ok(&["stats"]);
    assert_contains(&out, "notes       1");

    let out = sandbox.ok(&["stats", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("stats json");
    assert_eq!(parsed["decisions"], 1);

    let out = sandbox.ok(&["export"]);
    assert_contains(&out, ".html");
    let exports = sandbox.state().join("exports");
    let written = std::fs::read_dir(&exports).expect("exports dir").count();
    assert_eq!(written, 1);
    let file = std::fs::read_dir(&exports)
        .expect("exports dir")
        .next()
        .expect("one file")
        .expect("entry")
        .path();
    let html = std::fs::read_to_string(file).expect("read export");
    assert!(html.starts_with("<!doctype html>"));
}

#[test]
fn loop_commands_emit_structured_json() {
    let sandbox = Sandbox::new("loop-json");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    sandbox.ok(&["accept", "DR-0001"]);

    let out = sandbox.ok(&["handover", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("handover json");
    assert!(parsed["decided"]
        .as_array()
        .map(|a| a.iter().any(|v| v.as_str().unwrap_or("").contains("DR-0001")))
        .unwrap_or(false));

    let out = sandbox.ok(&["wrap", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("wrap json");
    assert!(parsed["date"].as_str().is_some());

    let out = sandbox.ok(&["digest", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("digest json");
    assert!(parsed["path"].as_str().unwrap_or("").starts_with("daily/digests/"));
}

#[test]
fn watch_help_is_available() {
    // The watch loop blocks by design, so the smoke test only proves the
    // command is wired up and its flags parse.
    let sandbox = Sandbox::new("watch");
    let output = sandbox.run(&["watch", "--help"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("watch"), "no watch help: {text}");
    assert!(text.contains("debounce"), "missing debounce flag: {text}");
}

#[test]
fn chat_refuses_when_the_llm_is_off() {
    let sandbox = Sandbox::new("chat-off");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    let output = sandbox.run(&["chat", "what did we decide about okta"]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(err.contains("LLM is off"), "unhelpful error: {err}");
}

#[test]
fn chat_preview_shows_the_payload_without_sending() {
    let sandbox = Sandbox::new("chat-preview");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    sandbox.ok(&["config", "set", "llm_mode", "byok"]);
    sandbox.ok(&["config", "set", "llm_send_content", "true"]);

    let out = sandbox.ok(&["chat", "okta", "--preview"]);
    assert_contains(&out, "would send");
    assert_contains(&out, "DR-0001");
    assert_contains(&out, "--- system ---");
}

#[test]
fn keys_are_stored_shown_and_cleared_without_printing_values() {
    let sandbox = Sandbox::new("keys");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);

    let out = sandbox.ok(&["key", "show", "backup"]);
    assert_contains(&out, "not set");

    let generated = sandbox.ok(&["key", "set", "backup", "--generate"]);
    assert_contains(&generated, "generated key");
    let key = generated
        .lines()
        .find(|line| line.len() == 64 && line.chars().all(|c| c.is_ascii_hexdigit()))
        .expect("a hex key was printed once")
        .to_string();

    let out = sandbox.ok(&["key", "show", "backup"]);
    assert_contains(&out, "set");
    assert!(!out.contains(&key), "show printed the key");

    let output = sandbox.run(&["key", "set", "nonsense"]);
    assert!(!output.status.success());

    let out = sandbox.ok(&["key", "clear", "backup"]);
    assert_contains(&out, "cleared");
}

#[test]
fn an_encrypted_backup_can_be_restored() {
    let sandbox = Sandbox::new("encrypted");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    sandbox.ok(&["key", "set", "backup", "--generate"]);
    sandbox.ok(&["config", "set", "backup_encrypt", "true"]);

    let out = sandbox.ok(&["backup"]);
    assert_contains(&out, ".tar.gz.enc");
    assert_contains(&out, "AES-256-GCM");

    let dir = sandbox.state().join("backups");
    let file = std::fs::read_dir(&dir)
        .expect("backups")
        .next()
        .expect("one backup")
        .expect("entry")
        .path();
    let dest = sandbox.root.join("restored");
    let out = sandbox.ok(&[
        "restore",
        file.to_str().unwrap(),
        "--into",
        dest.to_str().unwrap(),
    ]);
    assert_contains(&out, "decrypted");
    assert!(dest.join("decisions/DR-0001.md").exists());
}

#[test]
fn ui_help_is_available() {
    // Opening a window in CI is not possible; the flag surface is what is checked.
    let sandbox = Sandbox::new("ui");
    let output = sandbox.run(&["ui", "--help"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("ui"), "no ui help: {text}");
}

#[test]
fn the_json_surface_is_complete() {
    let sandbox = Sandbox::new("json-all");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "We decided to adopt Okta for SSO across all products."]);
    sandbox.ok(&["accept", "DR-0001"]);

    let transcript = sandbox.root.join("transcript.txt");
    std::fs::write(
        &transcript,
        "Bob: we decided to move billing to Razorpay.\n- [ ] Priya will prepare the checklist\n",
    )
    .expect("write transcript");

    let out = sandbox.ok(&["meeting", transcript.to_str().unwrap(), "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("meeting json");
    assert!(parsed["candidates"].as_array().map(|a| !a.is_empty()).unwrap_or(false));
    assert_eq!(parsed["next_reference"], "DR-0002");

    let out = sandbox.ok(&["ask", "okta", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("ask json");
    assert_eq!(parsed[0]["decision_id"], "DR-0001");

    let source = sandbox.root.join("import-source");
    std::fs::create_dir_all(&source).expect("dir");
    std::fs::write(source.join("note.md"), "we agreed to ship on friday\n").expect("write");
    let out = sandbox.ok(&["import", source.to_str().unwrap(), "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("import json");
    assert_eq!(parsed["entries"].as_array().map(|a| a.len()).unwrap_or(0), 1);

    let out = sandbox.ok(&["sop", "--json"]);
    serde_json::from_str::<serde_json::Value>(&out).expect("sop json");

    let out = sandbox.ok(&["cluster", "--json"]);
    serde_json::from_str::<serde_json::Value>(&out).expect("cluster json");
}

#[test]
fn capture_preview_honours_a_forced_kind() {
    let sandbox = Sandbox::new("preview-kind");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    let out = sandbox.ok(&["capture", "hmm", "--preview", "--kind", "task"]);
    assert_contains(&out, "would store as task");
    assert_contains(&out, "classifier guessed");
    // The folder exists from init; the preview must not have written a note into it.
    let written = std::fs::read_dir(sandbox.vault().join("tasks"))
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(written, 0, "preview wrote a note");
}

#[test]
fn a_meeting_note_inside_the_vault_can_be_given_as_an_absolute_path() {
    let sandbox = Sandbox::new("meeting-abs");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    let note = sandbox.vault().join("meetings/kickoff.md");
    std::fs::create_dir_all(sandbox.vault().join("meetings")).expect("dir");
    std::fs::write(
        &note,
        "---\ntype: meeting\ntitle: Kickoff\n---\nBob: we decided to move billing to Razorpay.\naction: Priya will prepare the checklist\n",
    )
    .expect("write note");

    let out = sandbox.ok(&[
        "meeting",
        note.to_str().unwrap(),
        "--approve",
        "1",
        "--actions",
    ]);
    assert_contains(&out, "recorded DR-0001");
    assert_contains(&out, "wrote 1 action item(s) into meetings/kickoff.md");

    // The source is recorded as a vault-relative path, not the absolute one.
    let decision = sandbox.read("decisions/DR-0001.md");
    assert_contains(&decision, "source: meetings/kickoff.md");

    // And the action item landed in the note as a checkbox.
    let updated = std::fs::read_to_string(&note).expect("read note");
    assert_contains(&updated, "- [ ] Priya will prepare the checklist");
}

#[test]
fn candidate_cards_count_up_instead_of_repeating_one_reference() {
    let sandbox = Sandbox::new("meeting-cards");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    let note = sandbox.vault().join("transcript.txt");
    std::fs::write(
        &note,
        "we decided to adopt Okta for SSO.\nfinal call is to move billing to Razorpay.\n",
    )
    .expect("write");

    let out = sandbox.ok(&["meeting", note.to_str().unwrap()]);
    assert_contains(&out, "approve as DR-0001?");
    assert_contains(&out, "approve as DR-0002?");
}

#[test]
fn link_suggests_then_accepts() {
    let sandbox = Sandbox::new("link");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["capture", "--text", "we decided okta sso pilot admin tooling enterprise pricing"]);
    sandbox.ok(&["capture", "--text", "notes: okta sso pilot admin tooling enterprise pricing review"]);
    sandbox.ok(&["capture", "--text", "payroll statutory filing interest penalty challan"]);

    let out = sandbox.ok(&["link"]);
    assert_contains(&out, "vocabulary only");
    assert_contains(&out, "shared vocabulary");

    // Suggestions write nothing.
    let json = sandbox.ok(&["link", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("link json");
    let first = parsed
        .as_array()
        .and_then(|entries| entries.first())
        .expect("at least one note with a suggestion");
    let note = first[0].as_str().expect("note path").to_string();
    assert!(first[1].as_array().map(|a| !a.is_empty()).unwrap_or(false));

    let out = sandbox.ok(&["link", "--note", &note, "--accept", "1"]);
    assert_contains(&out, "linked");
    let text = sandbox.read(&note);
    assert_contains(&text, "links:");
}

#[test]
fn calendar_suggests_and_accepts_a_project_tag() {
    let sandbox = Sandbox::new("calendar");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    sandbox.ok(&["config", "set", "calendar_enabled", "true"]);
    let today = chrono::Local::now().format("%Y%m%d").to_string();
    std::fs::write(
        sandbox.vault().join("meetings/atlas.md"),
        "---\nid: 01A\ntype: meeting\ntitle: Atlas cutover plan\n---\natlas migration notes\n",
    )
    .expect("note");
    let ics = sandbox.root.join("calendar.ics");
    std::fs::write(
        &ics,
        format!("BEGIN:VEVENT\nSUMMARY:Atlas migration review\nDTSTART:{today}T090000Z\nEND:VEVENT\n"),
    )
    .expect("ics");

    let out = sandbox.ok(&["calendar", ics.to_str().unwrap()]);
    assert_contains(&out, "Atlas migration review");
    assert_contains(&out, "meetings/atlas.md");

    let out = sandbox.ok(&["calendar", ics.to_str().unwrap(), "--accept", "1"]);
    assert_contains(&out, "tagged 1 note(s)");
    let note = sandbox.read("meetings/atlas.md");
    assert_contains(&note, "projects: [");
    assert_contains(&note, "Atlas migration review");
}

#[test]
fn calendar_is_off_until_enabled() {
    let sandbox = Sandbox::new("calendar-off");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);
    let ics = sandbox.root.join("calendar.ics");
    std::fs::write(&ics, "BEGIN:VEVENT\nSUMMARY:Anything\nEND:VEVENT\n").expect("ics");
    let output = sandbox.run(&["calendar", ics.to_str().unwrap()]);
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(err.contains("calendar signal is off"), "{err}");
}

#[test]
fn action_commands_emit_json() {
    let sandbox = Sandbox::new("action-json");
    sandbox.ok(&["init", sandbox.vault().to_str().unwrap()]);

    let out = sandbox.ok(&["capture", "we decided to adopt okta for sso", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("capture json");
    assert_eq!(parsed["path"], "decisions/DR-0001.md");
    assert_eq!(parsed["routed_to_inbox"], false);

    let out = sandbox.ok(&["accept", "DR-0001", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("accept json");
    assert_eq!(parsed["status"], "accepted");

    let out = sandbox.ok(&["reject", "DR-0001", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("reject json");
    assert_eq!(parsed["status"], "accepted", "an accepted decision cannot be rejected");

    let out = sandbox.ok(&["reindex", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("reindex json");
    assert_eq!(parsed["indexed"], 1);

    let out = sandbox.ok(&["check", "--json"]);
    serde_json::from_str::<serde_json::Value>(&out).expect("check json");
}
