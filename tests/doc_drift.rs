//! Documentation drift guards.
//!
//! A command that exists but is documented nowhere, or a module missing from the
//! architecture map, is the kind of gap a reviewer notices and a test should.

const LIB: &str = include_str!("../src/lib.rs");
const MAIN: &str = include_str!("../src/main.rs");
const README: &str = include_str!("../README.md");
const ARCHITECTURE: &str = include_str!("../docs/ARCHITECTURE.md");
const AGENTS: &str = include_str!("../AGENTS.md");

/// `pub mod name;` in lib.rs, skipping the feature-gated ones (they are still
/// documented, so they are included here too).
fn modules() -> Vec<String> {
    LIB.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("pub mod ")?;
            Some(rest.trim_end_matches(';').trim().to_string())
        })
        .collect()
}

/// Variants of the `Sub` enum, which is what a user types as a command.
fn top_level_commands() -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in MAIN.lines() {
        if line.contains("enum Sub {") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.starts_with('}') {
            break;
        }
        // `    Init(InitCmd),` — the command is the part before the parenthesis.
        let trimmed = line.trim().trim_end_matches(',');
        let mut parts = trimmed.split('(');
        let name = parts.next().unwrap_or("");
        if parts.next().is_some()
            && !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric())
        {
            out.push(name.to_ascii_lowercase());
        }
    }
    out
}

#[test]
fn every_module_is_in_the_architecture_map() {
    let modules = modules();
    assert!(modules.len() > 20, "parsed only {} modules", modules.len());
    let missing: Vec<&String> = modules
        .iter()
        .filter(|module| !ARCHITECTURE.contains(&format!("`{module}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "modules missing from docs/ARCHITECTURE.md: {missing:?}"
    );
}

#[test]
fn every_command_is_in_the_readme() {
    let commands = top_level_commands();
    assert!(commands.len() > 30, "parsed only {} commands", commands.len());
    let documented: Vec<String> = README
        .lines()
        .map(|line| line.trim().to_string())
        .collect();
    let missing: Vec<&String> = commands
        .iter()
        .filter(|command| {
            !documented
                .iter()
                .any(|line| line.split_whitespace().next() == Some(command.as_str()))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "commands with no line in README.md: {missing:?}"
    );
}

#[test]
fn the_agents_file_names_the_verification_commands() {
    for needle in [
        "cargo test --all-targets",
        "cargo clippy",
        "scripts/demo.sh",
        "no-default-features",
    ] {
        assert!(
            AGENTS.contains(needle),
            "AGENTS.md does not mention {needle}"
        );
    }
}

#[test]
fn the_readme_points_at_the_documents_that_exist() {
    for doc in [
        "docs/ARCHITECTURE.md",
        "docs/ONTOLOGY.md",
        "docs/ROADMAP.md",
        "docs/AUDIT.md",
        "docs/PACKAGING.md",
        "docs/USAGE.md",
        "CHANGELOG.md",
    ] {
        // `cargo test` runs with the crate root as the working directory.
        let exists = std::path::Path::new(doc).exists();
        assert!(exists, "{doc} is linked but missing");
    }
}
