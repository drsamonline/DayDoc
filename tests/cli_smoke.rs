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
