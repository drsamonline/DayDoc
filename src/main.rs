//! `daydoc` — the command line surface. The GUI calls the same library.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use argh::FromArgs;

use daydoc::error::DayDocError;
#[cfg(feature = "git")]
use daydoc::sync;
use daydoc::types::{DecisionStatus, NoteType};
use daydoc::{
    attach, backup, capture, cluster, crypto, daily, embed, export, handover, index, ledger, lint,
    link, llm, meeting, migrate, search, secret, sop, stale, stats, watch, Vault,
};

#[derive(FromArgs, Debug)]
/// decision-first office notes (DayDoc)
struct TopLevel {
    #[argh(option, short = 'v')]
    /// vault root (default: $DAYDOC_VAULT, then the current directory)
    vault: Option<String>,
    #[argh(switch)]
    /// open the vault read-only for this run
    read_only_vault: bool,
    #[argh(subcommand)]
    cmd: Sub,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
enum Sub {
    Init(InitCmd),
    Capture(CaptureCmd),
    Meeting(MeetingCmd),
    Decisions(DecisionsCmd),
    Show(ShowCmd),
    Accept(AcceptCmd),
    Reject(RejectCmd),
    Supersede(SupersedeCmd),
    Revert(RevertCmd),
    Search(SearchCmd),
    Ask(AskCmd),
    Stale(StaleCmd),
    Verify(VerifyCmd),
    Archive(ArchiveCmd),
    Handover(HandoverCmd),
    Wrap(WrapCmd),
    Digest(DigestCmd),
    Sop(SopCmd),
    Backup(BackupCmd),
    Restore(RestoreCmd),
    Key(KeyCmd),
    Config(ConfigCmd),
    Stats(StatsCmd),
    Export(ExportCmd),
    Check(CheckCmd),
    Import(ImportCmd),
    Lint(LintCmd),
    Link(LinkCmd),
    Sync(SyncCmd),
    Reindex(ReindexCmd),
    Ui(UiCmd),
    Watch(WatchCmd),
    Chat(ChatCmd),
    Cluster(ClusterCmd),
    Attach(AttachCmd),
    Purge(PurgeCmd),
    Replay(ReplayCmd),
    Status(StatusCmd),
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "init")]
/// create a vault in a folder; no wizard, no account
struct InitCmd {
    #[argh(positional)]
    /// folder to use as the vault
    path: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "capture")]
/// classify one capture and store it
struct CaptureCmd {
    #[argh(positional)]
    /// the text, a path to a file, or - for stdin
    input: Option<String>,
    #[argh(option)]
    /// the text, for captures that start with a dash
    text: Option<String>,
    #[argh(option)]
    /// force a type instead of classifying
    kind: Option<String>,
    #[argh(switch)]
    /// show what would happen, write nothing
    preview: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "meeting")]
/// extract decision candidates from a transcript or meeting note
struct MeetingCmd {
    #[argh(positional)]
    /// path to the transcript or note
    file: String,
    #[argh(switch)]
    /// print json instead of cards
    json: bool,
    #[argh(option)]
    /// approve these candidates, 1-based, comma separated
    approve: Option<String>,
    #[argh(switch)]
    /// approve every candidate
    all: bool,
    #[argh(switch)]
    /// also write extracted action items into the source note
    actions: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "decisions")]
/// list the decision ledger as cards
struct DecisionsCmd {
    #[argh(option)]
    /// filter by status: proposed, accepted, superseded, rejected
    status: Option<String>,
    #[argh(switch)]
    /// print json instead of cards
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "show")]
/// show one decision, with its supersession chain
struct ShowCmd {
    #[argh(positional)]
    /// decision reference: DR-NNNN, a ULID, or a title
    reference: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "accept")]
/// accept a proposed decision
struct AcceptCmd {
    #[argh(positional)]
    /// decision reference, DR-NNNN
    reference: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "reject")]
/// reject a proposed decision
struct RejectCmd {
    #[argh(positional)]
    /// decision reference, DR-NNNN
    reference: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "supersede")]
/// record a new decision that replaces an older one
struct SupersedeCmd {
    #[argh(positional)]
    /// the decision being replaced, DR-NNNN
    reference: String,
    #[argh(positional)]
    /// title of the new decision
    title: String,
    #[argh(option)]
    /// what was decided, in a sentence
    decision: Option<String>,
    #[argh(option)]
    /// who owns it
    owner: Option<String>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "revert")]
/// undo a supersession that should not have happened
struct RevertCmd {
    #[argh(positional)]
    /// the decision that did the superseding, DR-NNNN
    reference: String,
    #[argh(option)]
    /// why it is being reverted
    reason: Option<String>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "search")]
/// fuzzy search across titles, bodies, entities and tags
struct SearchCmd {
    #[argh(positional)]
    /// what to look for
    query: String,
    #[argh(switch)]
    /// print json instead of hits
    json: bool,
    #[argh(option)]
    /// filter by note type
    kind: Option<String>,
    #[argh(option)]
    /// filter by person
    person: Option<String>,
    #[argh(option)]
    /// filter by project
    project: Option<String>,
    #[argh(option)]
    /// filter by status
    status: Option<String>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "ask")]
/// ask about a decision and get the card, not the prose
struct AskCmd {
    #[argh(positional)]
    /// the question
    question: String,
    #[argh(switch)]
    /// print json instead of cards
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "stale")]
/// show every note that is stale or contradicts the ledger
struct StaleCmd {
    #[argh(switch)]
    /// print json instead of flags
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "verify")]
/// re-verify a note: still true, stamp today
struct VerifyCmd {
    #[argh(positional)]
    /// vault-relative path of the note
    path: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "archive")]
/// move a note to _archive/YYYY/
struct ArchiveCmd {
    #[argh(positional)]
    /// vault-relative path of the note
    path: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "handover")]
/// compile a handover packet for a period
struct HandoverCmd {
    #[argh(option)]
    /// start date, YYYY-MM-DD (default: 14 days ago)
    from: Option<String>,
    #[argh(option)]
    /// end date, YYYY-MM-DD (default: today)
    to: Option<String>,
    #[argh(switch)]
    /// print only, write nothing
    dry_run: bool,
    #[argh(switch)]
    /// print the packet as json
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "wrap")]
/// end-of-day wrap for a date
struct WrapCmd {
    #[argh(option)]
    /// date, YYYY-MM-DD (default: today)
    date: Option<String>,
    #[argh(switch)]
    /// print only, write nothing
    dry_run: bool,
    #[argh(switch)]
    /// print the wrap as json
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "digest")]
/// weekly digest for the week containing a date
struct DigestCmd {
    #[argh(option)]
    /// date, YYYY-MM-DD (default: today)
    date: Option<String>,
    #[argh(switch)]
    /// print only, write nothing
    dry_run: bool,
    #[argh(switch)]
    /// print the digest as json
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "sop")]
/// propose SOPs from repeated procedures, or accept one
struct SopCmd {
    #[argh(switch)]
    /// accept a proposal instead of listing
    accept: bool,
    #[argh(switch)]
    /// print json instead of a listing
    json: bool,
    #[argh(option)]
    /// proposal number to accept, 1-based
    index: Option<usize>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "link")]
/// suggest links between notes, and accept one
struct LinkCmd {
    #[argh(option)]
    /// only this note
    note: Option<String>,
    #[argh(option)]
    /// accept suggestion N for --note, 1-based
    accept: Option<usize>,
    #[argh(option)]
    /// suggestions per note (default: 3)
    limit: Option<usize>,
    #[argh(option)]
    /// minimum similarity, 0 to 1 (default: 0.55)
    min_score: Option<f32>,
    #[argh(switch)]
    /// print json
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "sync")]
/// commit the vault to git (never pushes)
struct SyncCmd {
    #[argh(option)]
    /// commit message
    message: Option<String>,
    #[argh(switch)]
    /// show what would be committed, write nothing
    dry_run: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "restore")]
/// unpack a backup into a folder
struct RestoreCmd {
    #[argh(positional)]
    /// the .tar.gz or .tar.gz.enc file
    file: String,
    #[argh(option)]
    /// where to unpack it (default: the state directory)
    into: Option<String>,
    #[argh(switch)]
    /// unpack into a folder that already has files
    force: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "key")]
/// store, show or clear a key (values are never printed)
struct KeyCmd {
    #[argh(subcommand)]
    action: KeyAction,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
enum KeyAction {
    Set(KeySet),
    Show(KeyShow),
    Clear(KeyClear),
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "set")]
/// store a key: read from stdin, or generate one
struct KeySet {
    #[argh(positional)]
    /// which key: backup or llm
    name: String,
    #[argh(switch)]
    /// generate a random 32-byte key and print it once
    generate: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "show")]
/// report whether a key is set
struct KeyShow {
    #[argh(positional)]
    /// which key: backup or llm
    name: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "clear")]
/// remove a stored key
struct KeyClear {
    #[argh(positional)]
    /// which key: backup or llm
    name: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "config")]
/// show or change daydoc.toml
struct ConfigCmd {
    #[argh(subcommand)]
    action: ConfigAction,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
enum ConfigAction {
    Show(ConfigShow),
    Set(ConfigSet),
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "show")]
/// print the effective configuration
struct ConfigShow {
    #[argh(switch)]
    /// print json instead of toml
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "set")]
/// change one setting and write daydoc.toml
struct ConfigSet {
    #[argh(positional)]
    /// the key, for example stale.sop
    key: String,
    #[argh(positional)]
    /// the new value
    value: String,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "stats")]
/// vault counts, entities and dates
struct StatsCmd {
    #[argh(switch)]
    /// print json instead of a summary
    json: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "export")]
/// write a period as one self-contained html file
struct ExportCmd {
    #[argh(option)]
    /// start date, YYYY-MM-DD (default: 30 days ago)
    from: Option<String>,
    #[argh(option)]
    /// end date, YYYY-MM-DD (default: today)
    to: Option<String>,
    #[argh(option)]
    /// where to write the file (default: the state directory)
    out: Option<String>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "lint")]
/// report vault problems: ids, links, citations, assets
struct LintCmd {
    #[argh(switch)]
    /// print json instead of lines
    json: bool,
    #[argh(switch)]
    /// assign ids to notes that have none, then report
    fix: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "ui")]
/// open the desktop window
struct UiCmd {}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "chat")]
/// ask the optional llm about the vault (off by default)
struct ChatCmd {
    #[argh(positional)]
    /// the question
    question: String,
    #[argh(option)]
    /// how many notes to retrieve (default: 6)
    k: Option<usize>,
    #[argh(switch)]
    /// show exactly what would be sent, send nothing
    preview: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "watch")]
/// watch the vault: reindex on external edits, commit on a timer
struct WatchCmd {
    #[argh(option)]
    /// debounce window in milliseconds (default: 500)
    debounce_ms: Option<u64>,
    #[argh(switch)]
    /// reindex only, never commit
    no_commit: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "reindex")]
/// rebuild the search index from the vault
struct ReindexCmd {}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "backup")]
/// write a dated .tar.gz of the vault and apply retention
struct BackupCmd {
    #[argh(option)]
    /// folder to write backups into (default: the state directory)
    out: Option<String>,
    #[argh(switch)]
    /// show the plan, write nothing
    dry_run: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "check")]
/// identity check plus trash purge in one pass
struct CheckCmd {}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "import")]
/// import an existing Markdown folder, with a dry-run preview
struct ImportCmd {
    #[argh(positional)]
    /// folder of Markdown files to import
    path: String,
    #[argh(switch)]
    /// print json instead of a report
    json: bool,
    #[argh(switch)]
    /// show what would happen, write nothing
    dry_run: bool,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "cluster")]
/// propose project folders from notes that already look like one project
struct ClusterCmd {
    #[argh(switch)]
    /// accept a proposal instead of listing
    accept: bool,
    #[argh(switch)]
    /// print json instead of a listing
    json: bool,
    #[argh(option)]
    /// proposal number to accept, 1-based
    index: Option<usize>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "attach")]
/// store a file content-addressed under assets/, optionally linking it from a note
struct AttachCmd {
    #[argh(positional)]
    /// file to store
    file: String,
    #[argh(option)]
    /// vault-relative note to reference the file from
    note: Option<String>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "purge")]
/// delete trash entries past the retention window
struct PurgeCmd {}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "replay")]
/// replay staged writes left behind by a crash
struct ReplayCmd {}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "status")]
/// vault counts and open threads
struct StatusCmd {
    #[argh(switch)]
    /// print json instead of a summary
    json: bool,
}

fn main() -> ExitCode {
    let args: TopLevel = argh::from_env();
    match run(args) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("daydoc: {err}");
            ExitCode::FAILURE
        }
    }
}

fn open_vault(explicit: Option<String>, read_only: bool) -> daydoc::Result<Vault> {
    let root = explicit
        .or_else(|| std::env::var("DAYDOC_VAULT").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut vault = Vault::open(&root)?;
    if read_only {
        vault.config_mut().read_only_vault = true;
    }
    for warning in vault.replay_wal()? {
        eprintln!("daydoc: {warning}");
    }
    // The index is an optimisation, never a source of truth: refresh it when it
    // disagrees with the vault and carry on if it cannot be built.
    let _ = index::ensure(&vault);
    Ok(vault)
}

fn read_text(arg: &str) -> daydoc::Result<String> {
    if arg == "-" {
        let mut buffer = String::new();
        use std::io::Read;
        std::io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| DayDocError::Io {
                path: "stdin".to_string(),
                source: e,
            })?;
        return Ok(buffer);
    }
    let path = PathBuf::from(arg);
    if path.is_file() {
        return std::fs::read_to_string(&path).map_err(|e| DayDocError::Io {
            path: arg.to_string(),
            source: e,
        });
    }
    Ok(arg.to_string())
}

fn parse_kind(raw: &str) -> daydoc::Result<NoteType> {
    NoteType::parse(raw).ok_or_else(|| {
        DayDocError::Config(format!(
            "unknown note type \"{raw}\"; try daily, meeting, decision, task, idea, sop, project-update, reference"
        ))
    })
}

fn run_sync(
    vault: &Vault,
    message: Option<String>,
    dry_run: bool,
) -> daydoc::Result<ExitCode> {
    #[cfg(feature = "git")]
    {
        if dry_run {
            let changed = sync::status(vault)?;
            if changed.is_empty() {
                println!("nothing to commit");
            } else {
                println!("{} file(s) would be committed:", changed.len());
                for path in changed.iter().take(40) {
                    println!("  {path}");
                }
            }
            return Ok(ExitCode::SUCCESS);
        }
        let conflicts = sync::quarantine_conflicts(vault)?;
        for sidecar in &conflicts {
            println!("conflict copy written to {sidecar}");
        }
        let report = sync::commit(vault, message.as_deref())?;
        print!("{}", report.render());
        Ok(ExitCode::SUCCESS)
    }
    #[cfg(not(feature = "git"))]
    {
        let _ = (vault, message, dry_run);
        Err(DayDocError::Config(
            "this build has no git support (feature \"git\" is off)".to_string(),
        ))
    }
}

/// A path inside the vault is used as a vault-relative path, so `--actions` can
/// write into it. A transcript outside the vault keeps its own path, as
/// provenance for the decisions it produced.
fn resolve_source(vault: &Vault, arg: &str) -> String {
    let path = PathBuf::from(arg);
    if path.is_absolute() {
        // Both sides are canonicalised: on Windows `canonicalize` returns a
        // verbatim path (`\\?\C:\...`) that will not strip against the plain
        // vault root.
        if let (Ok(root), Ok(full)) = (vault.root().canonicalize(), path.canonicalize()) {
            if let Ok(rest) = full.strip_prefix(&root) {
                let rel = rest
                    .components()
                    .filter_map(|component| match component {
                        std::path::Component::Normal(part) => {
                            Some(part.to_string_lossy().to_string())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("/");
                if !rel.is_empty() {
                    return rel;
                }
            }
        }
    }
    arg.to_string()
}

fn advance_reference(reference: &str, by: u64) -> String {
    match daydoc::ids::parse_decision_id(reference) {
        Some(number) => daydoc::ids::decision_id(number + by),
        None => reference.to_string(),
    }
}

fn normalise_key_name(raw: &str) -> daydoc::Result<String> {
    let name = raw.trim().to_ascii_lowercase();
    if matches!(name.as_str(), "backup" | "llm") {
        Ok(name)
    } else {
        Err(DayDocError::Config(format!(
            "unknown key \"{raw}\"; use backup or llm"
        )))
    }
}

fn print_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(err) => eprintln!("daydoc: could not serialise: {err}"),
    }
}

fn today_or(raw: Option<String>) -> String {
    raw.unwrap_or_else(daydoc::vault::today)
}

fn run(args: TopLevel) -> daydoc::Result<ExitCode> {
    let explicit = args.vault.clone();
    let read_only = args.read_only_vault;
    match args.cmd {
        Sub::Init(cmd) => {
            let vault = Vault::init(&PathBuf::from(&cmd.path))?;
            println!("vault ready at {}", vault.root().display());
            println!("  {} note(s) already here", vault.note_count()?);
            Ok(ExitCode::SUCCESS)
        }

        Sub::Capture(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let raw = match (cmd.input, cmd.text) {
                (Some(input), _) => input,
                (None, Some(text)) => text,
                (None, None) => {
                    return Err(DayDocError::Config(
                        "nothing to capture: pass the text, --text \"…\", or - for stdin"
                            .to_string(),
                    ))
                }
            };
            let text = read_text(&raw)?;
            if cmd.preview {
                match cmd.kind.as_deref() {
                    Some(kind) => {
                        let forced = parse_kind(kind)?;
                        let classification = capture::preview(&text);
                        println!(
                            "would store as {forced} (--kind), classifier guessed {} at {:.0}%",
                            classification.note_type,
                            classification.confidence * 100.0
                        );
                    }
                    None => {
                        let classification = capture::preview(&text);
                        println!(
                            "would route as {} at {:.0}% (confident: {})",
                            classification.note_type,
                            classification.confidence * 100.0,
                            classification
                                .is_confident(vault.config().min_classification_confidence)
                        );
                    }
                }
                return Ok(ExitCode::SUCCESS);
            }
            let outcome = match cmd.kind.as_deref() {
                Some(kind) => capture::capture_as(&vault, &text, parse_kind(kind)?)?,
                None => capture::capture(&vault, &text)?,
            };
            println!("{}", outcome.describe());
            Ok(ExitCode::SUCCESS)
        }

        Sub::Meeting(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let raw = read_text(&cmd.file)?;
            let source = resolve_source(&vault, &cmd.file);
            // A meeting note keeps its frontmatter; a raw transcript has none.
            let text = match daydoc::frontmatter::split(&raw) {
                Ok((_, body)) => body,
                Err(_) => raw,
            };
            let candidates = meeting::extract_decisions(&text);
            let actions = meeting::extract_action_items(&text);
            let next = meeting::next_reference(&vault);

            if cmd.json {
                print_json(&serde_json::json!({
                    "file": source,
                    "next_reference": next,
                    "candidates": candidates,
                    "action_items": actions,
                }));
                return Ok(ExitCode::SUCCESS);
            }

            println!("{} candidate(s) in {}", candidates.len(), source);
            for (i, candidate) in candidates.iter().enumerate() {
                // Each approved candidate takes the next number in turn.
                let reference = advance_reference(&next, i as u64);
                println!("  {}. {}", i + 1, candidate.card(&reference));
            }
            if !actions.is_empty() {
                println!("{} action item(s):", actions.len());
                for action in &actions {
                    println!("  - {action}");
                }
            }

            let mut chosen: Vec<meeting::Candidate> = Vec::new();
            if cmd.all {
                chosen = candidates.clone();
            } else if let Some(list) = &cmd.approve {
                for part in list.split(',') {
                    let index: usize = part.trim().parse().map_err(|_| {
                        DayDocError::Config(format!("not a candidate number: {part}"))
                    })?;
                    if index == 0 || index > candidates.len() {
                        return Err(DayDocError::Config(format!(
                            "candidate {index} is out of range (1..{})",
                            candidates.len()
                        )));
                    }
                    chosen.push(candidates[index - 1].clone());
                }
            }

            if chosen.is_empty() {
                println!("\nrun `daydoc meeting {} --approve 1,2` to record these", source);
                return Ok(ExitCode::SUCCESS);
            }
            let written = meeting::approve(&vault, &source, &chosen)?;
            for note in &written {
                println!(
                    "recorded {} — {}",
                    note.frontmatter.decision_id.as_deref().unwrap_or("?"),
                    note.title()
                );
            }
            if cmd.actions {
                if !vault.exists(&source) {
                    return Err(DayDocError::Config(format!(
                        "cannot write action items into {source}: it is not inside the vault. \
                         Copy it in first, or drop --actions."
                    )));
                }
                let added = meeting::append_action_items(&vault, &source, &actions)?;
                println!("wrote {added} action item(s) into {source}");
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Decisions(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let wanted = match cmd.status.as_deref() {
                Some(raw) => Some(DecisionStatus::parse(raw).ok_or_else(|| {
                    DayDocError::Config(format!("unknown status \"{raw}\""))
                })?),
                None => None,
            };
            let cards: Vec<ledger::DecisionCard> = ledger::cards(&vault)?
                .into_iter()
                .filter(|card| wanted.map(|w| card.status == w).unwrap_or(true))
                .collect();
            if cmd.json {
                print_json(&cards);
                return Ok(ExitCode::SUCCESS);
            }
            if cards.is_empty() {
                println!("no decisions yet — `daydoc capture \"we decided to ...\"`");
                return Ok(ExitCode::SUCCESS);
            }
            for card in &cards {
                print!("{}", card.render());
                println!();
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Show(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let note = ledger::find(&vault, &cmd.reference)?;
            let citations = ledger::citation_counts(&vault)?;
            let card = ledger::card(&note, &citations);
            print!("{}", card.render());
            println!("  file: {}", note.rel_path);
            println!();
            println!("{}", note.body.trim());
            Ok(ExitCode::SUCCESS)
        }

        Sub::Accept(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let note = ledger::accept(&vault, &cmd.reference)?;
            println!(
                "{} is now {}",
                note.frontmatter.decision_id.as_deref().unwrap_or("?"),
                ledger::status_of(&note)
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Reject(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let note = ledger::reject(&vault, &cmd.reference)?;
            println!(
                "{} is now {}",
                note.frontmatter.decision_id.as_deref().unwrap_or("?"),
                ledger::status_of(&note)
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Supersede(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let mut request = ledger::NewDecision::new(cmd.title.clone());
            if let Some(decision) = &cmd.decision {
                request.decision = decision.clone();
            }
            if let Some(owner) = &cmd.owner {
                request.owner = Some(owner.clone());
            }
            let note = ledger::supersede(&vault, &cmd.reference, &request)?;
            println!(
                "recorded {} — {} (supersedes {})",
                note.frontmatter.decision_id.as_deref().unwrap_or("?"),
                note.title(),
                note.frontmatter.supersedes.as_deref().unwrap_or("?")
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Revert(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let reason = cmd.reason.unwrap_or_else(|| "not given".to_string());
            let (new, old) = ledger::revert_supersession(&vault, &cmd.reference, &reason)?;
            println!(
                "reverted: {} and {} are both accepted again",
                old.frontmatter.decision_id.as_deref().unwrap_or("?"),
                new.frontmatter.decision_id.as_deref().unwrap_or("?")
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Search(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let mut filters = search::Filters::default();
            if let Some(kind) = cmd.kind.as_deref() {
                filters.note_type = Some(parse_kind(kind)?);
            }
            filters.person = cmd.person;
            filters.project = cmd.project;
            filters.status = cmd.status;
            let hits = search::search(&vault, &cmd.query, &filters)?;
            if cmd.json {
                print_json(&hits);
                return Ok(ExitCode::SUCCESS);
            }
            if hits.is_empty() {
                println!("no matches");
                return Ok(ExitCode::SUCCESS);
            }
            for hit in hits.iter().take(25) {
                print!("{}", hit.render());
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Ask(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let cards = search::ask(&vault, &cmd.question)?;
            if cmd.json {
                print_json(&cards);
                return Ok(ExitCode::SUCCESS);
            }
            if cards.is_empty() {
                println!("nothing in the ledger answers that");
                return Ok(ExitCode::SUCCESS);
            }
            for card in cards.iter().take(5) {
                print!("{}", card.render());
                println!();
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Stale(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let flags = stale::scan(&vault)?;
            if cmd.json {
                print_json(&flags);
                return Ok(ExitCode::SUCCESS);
            }
            if flags.is_empty() {
                println!("nothing is stale");
                return Ok(ExitCode::SUCCESS);
            }
            for flag in &flags {
                print!("{}", flag.render());
                println!();
            }
            println!("resolve with: daydoc verify <path> | daydoc archive <path>");
            Ok(ExitCode::SUCCESS)
        }

        Sub::Verify(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let note = stale::reverify(&vault, &cmd.path)?;
            println!(
                "re-verified {} ({})",
                note.rel_path,
                note.frontmatter.last_verified.as_deref().unwrap_or("today")
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Archive(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let dest = stale::archive(&vault, &cmd.path)?;
            println!("archived to {dest}");
            Ok(ExitCode::SUCCESS)
        }

        Sub::Handover(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let to = today_or(cmd.to);
            let from = cmd.from.unwrap_or_else(|| {
                let date = chrono::Local::now().date_naive() - chrono::Duration::days(14);
                date.format("%Y-%m-%d").to_string()
            });
            if cmd.json {
                print_json(&handover::build_data(&vault, &from, &to)?);
                return Ok(ExitCode::SUCCESS);
            }
            let packet = handover::build(&vault, &from, &to)?;
            if cmd.dry_run {
                print!("{}", packet.markdown);
            } else {
                let rel = handover::write(&vault, &packet)?;
                println!("handover written to {rel}");
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Wrap(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let date = today_or(cmd.date);
            if cmd.json {
                print_json(&daily::wrap_data(&vault, &date)?);
                return Ok(ExitCode::SUCCESS);
            }
            if cmd.dry_run {
                print!("{}", daily::eod_wrap(&vault, &date)?);
            } else {
                let rel = daily::write_wrap(&vault, &date)?;
                println!("wrap written into {rel}");
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Digest(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let date = today_or(cmd.date);
            if cmd.json {
                let (rel, data) = daily::digest_data(&vault, &date)?;
                print_json(&serde_json::json!({ "path": rel, "digest": data }));
                return Ok(ExitCode::SUCCESS);
            }
            if cmd.dry_run {
                let (_, md) = daily::weekly_digest(&vault, &date)?;
                print!("{md}");
            } else {
                let rel = daily::write_digest(&vault, &date)?;
                println!("digest written to {rel}");
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Sop(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let proposals = sop::propose(&vault)?;
            if cmd.json {
                print_json(&proposals);
                return Ok(ExitCode::SUCCESS);
            }
            if proposals.is_empty() {
                println!("no repeated procedures found (3+ notes must share a sequence)");
                return Ok(ExitCode::SUCCESS);
            }
            if !cmd.accept {
                for (i, proposal) in proposals.iter().enumerate() {
                    println!("{}. {}", i + 1, proposal.render());
                }
                println!("accept one with: daydoc sop --accept --index 1");
                return Ok(ExitCode::SUCCESS);
            }
            let index = cmd.index.unwrap_or(1);
            if index == 0 || index > proposals.len() {
                return Err(DayDocError::Config(format!(
                    "proposal {index} is out of range (1..{})",
                    proposals.len()
                )));
            }
            let note = sop::accept(&vault, &proposals[index - 1])?;
            println!("wrote {}", note.rel_path);
            Ok(ExitCode::SUCCESS)
        }

        Sub::Restore(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let file = PathBuf::from(&cmd.file);
            let dest = cmd
                .into
                .map(PathBuf::from)
                .unwrap_or_else(|| vault.state_dir().join("restore"));
            let key = secret::resolve(&vault, secret::BACKUP);
            let report = backup::restore(&file, &dest, key.as_deref(), cmd.force)?;
            println!(
                "restored {} file(s) into {}{}",
                report.files,
                dest.display(),
                if report.encrypted { " (decrypted)" } else { "" }
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Key(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            match cmd.action {
                KeyAction::Set(set) => {
                    let name = normalise_key_name(&set.name)?;
                    let value = if set.generate {
                        let generated = crypto::new_key_hex()?;
                        println!("generated key — copy it somewhere else too:");
                        println!("{generated}");
                        generated
                    } else {
                        use std::io::Read;
                        let mut buffer = String::new();
                        std::io::stdin().read_to_string(&mut buffer).map_err(|err| {
                            DayDocError::Io {
                                path: "stdin".to_string(),
                                source: err,
                            }
                        })?;
                        buffer.trim().to_string()
                    };
                    let location = secret::set(&vault, &name, &value)?;
                    println!("{name} key stored in {}", location.describe());
                    Ok(ExitCode::SUCCESS)
                }
                KeyAction::Show(show) => {
                    let name = normalise_key_name(&show.name)?;
                    match secret::location(&vault, &name) {
                        Some(place) => println!("{name} key: set in {place}"),
                        None => println!("{name} key: not set"),
                    }
                    Ok(ExitCode::SUCCESS)
                }
                KeyAction::Clear(clear) => {
                    let name = normalise_key_name(&clear.name)?;
                    let removed = secret::clear(&vault, &name)?;
                    println!("{}", if removed { "cleared" } else { "nothing to clear" });
                    Ok(ExitCode::SUCCESS)
                }
            }
        }

        Sub::Config(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            match cmd.action {
                ConfigAction::Show(show) => {
                    if show.json {
                        print_json(vault.config());
                    } else {
                        print!("{}", vault.config().to_toml());
                    }
                    Ok(ExitCode::SUCCESS)
                }
                ConfigAction::Set(set) => {
                    if vault.is_read_only() {
                        return Err(DayDocError::ReadOnly("config".to_string()));
                    }
                    let mut config = vault.config().clone();
                    let stored = config.set_key(&set.key, &set.value)?;
                    config.validate()?;
                    config.write_to(vault.root())?;
                    println!("{} = {stored}", set.key);
                    Ok(ExitCode::SUCCESS)
                }
            }
        }

        Sub::Stats(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let summary = stats::compute(&vault)?;
            if cmd.json {
                print_json(&summary);
            } else {
                print!("{}", summary.render());
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Export(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let to = today_or(cmd.to);
            let from = cmd.from.unwrap_or_else(|| {
                let date = chrono::Local::now().date_naive() - chrono::Duration::days(30);
                date.format("%Y-%m-%d").to_string()
            });
            let out = cmd.out.map(PathBuf::from).unwrap_or_else(|| {
                vault
                    .state_dir()
                    .join("exports")
                    .join(format!("daydoc-{from}-to-{to}.html"))
            });
            let report = export::run(&vault, &from, &to, &out)?;
            print!("{}", report.render());
            Ok(ExitCode::SUCCESS)
        }

        Sub::Link(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let embedder = embed::HashEmbedder::default();
            let limit = cmd.limit.unwrap_or(3);
            let min_score = cmd.min_score.unwrap_or_else(link::default_min_score);

            if let (Some(note), Some(index)) = (cmd.note.clone(), cmd.accept) {
                let suggestions = link::suggest_for(&vault, &embedder, &note, limit, min_score)?;
                let chosen = suggestions.get(index.saturating_sub(1)).ok_or_else(|| {
                    DayDocError::Config(format!(
                        "suggestion {index} is out of range ({} found)",
                        suggestions.len()
                    ))
                })?;
                let target = chosen.rel_path.clone();
                let updated = link::accept(&vault, &note, &target)?;
                println!(
                    "linked {} → {} ({} link(s) now)",
                    note,
                    target,
                    updated.frontmatter.links.len()
                );
                return Ok(ExitCode::SUCCESS);
            }

            if let Some(note) = cmd.note.clone() {
                let suggestions = link::suggest_for(&vault, &embedder, &note, limit, min_score)?;
                if cmd.json {
                    print_json(&suggestions);
                    return Ok(ExitCode::SUCCESS);
                }
                if suggestions.is_empty() {
                    println!("nothing worth linking to {note}");
                    return Ok(ExitCode::SUCCESS);
                }
                println!("suggestions for {note}:");
                for (i, suggestion) in suggestions.iter().enumerate() {
                    println!("  {}. {}", i + 1, suggestion.render().trim_end());
                }
                println!("accept one with: daydoc link --note {note} --accept 1");
                return Ok(ExitCode::SUCCESS);
            }

            let all = link::suggest_all(&vault, &embedder, limit, min_score)?;
            if cmd.json {
                print_json(&all);
                return Ok(ExitCode::SUCCESS);
            }
            if all.is_empty() {
                println!("no unlinked note has a suggestion");
                return Ok(ExitCode::SUCCESS);
            }
            for (note, suggestions) in &all {
                println!("{note}");
                for (i, suggestion) in suggestions.iter().enumerate() {
                    println!("  {}. {}", i + 1, suggestion.render().trim_end());
                }
            }
            println!(
                "\n{} — no embedding model is bundled, so these are vocabulary matches, \
                 not meaning. Dropping in the model changes nothing but src/embed.rs.",
                if embedder.is_semantic() {
                    "semantic"
                } else {
                    "vocabulary only"
                }
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Sync(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            run_sync(&vault, cmd.message, cmd.dry_run)
        }

        Sub::Lint(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            if cmd.fix {
                let report = lint::fix(&vault)?;
                print!("{}", report.render());
            }
            let issues = lint::scan(&vault)?;
            if cmd.json {
                print_json(&issues);
                return Ok(ExitCode::SUCCESS);
            }
            if issues.is_empty() {
                println!("vault is clean");
                return Ok(ExitCode::SUCCESS);
            }
            for issue in &issues {
                print!("{}", issue.render());
            }
            println!("\n{} issue(s)", issues.len());
            Ok(ExitCode::SUCCESS)
        }

        Sub::Chat(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let settings = llm::LlmSettings::from_config(vault.config());
            if !settings.enabled() {
                return Err(DayDocError::Llm(
                    "LLM is off. Every other feature works without it; to turn it on: \
                     `daydoc config set llm_mode byok`, then set DAYDOC_LLM_KEY."
                        .to_string(),
                ));
            }
            let k = cmd.k.unwrap_or(llm::DEFAULT_K);
            if cmd.preview {
                let plan = llm::plan(&vault, &cmd.question, k)?;
                println!("{}", plan.preview);
                println!("\n--- system ---\n{}\n--- user ---\n{}", plan.system, plan.user);
                return Ok(ExitCode::SUCCESS);
            }
            let answer = llm::ask_with(&vault, &cmd.question, k, &llm::HttpTransport)?;
            println!("{answer}");
            Ok(ExitCode::SUCCESS)
        }

        Sub::Ui(_) => {
            let vault = open_vault(explicit, read_only)?;
            #[cfg(feature = "ui")]
            {
                daydoc::ui::run(vault)?;
                Ok(ExitCode::SUCCESS)
            }
            #[cfg(not(feature = "ui"))]
            {
                let _ = vault;
                Err(DayDocError::Config(
                    "this build has no window (feature \"ui\" is off)".to_string(),
                ))
            }
        }

        Sub::Watch(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let mut config = watch::WatchConfig::default();
            if let Some(ms) = cmd.debounce_ms {
                config.debounce = Duration::from_millis(ms.max(50));
            }
            config.auto_commit = !cmd.no_commit && vault.config().auto_commit;
            config.commit_interval =
                Duration::from_secs(vault.config().auto_commit_debounce_secs.max(1));
            watch::run(&vault, &config)?;
            Ok(ExitCode::SUCCESS)
        }

        Sub::Reindex(_) => {
            let vault = open_vault(explicit, read_only)?;
            let report = index::reindex(&vault)?;
            print!("{}", report.render());
            Ok(ExitCode::SUCCESS)
        }

        Sub::Backup(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let dir = cmd
                .out
                .map(PathBuf::from)
                .unwrap_or_else(|| backup::default_dir(&vault));
            if cmd.dry_run {
                let existing = backup::existing(&dir);
                let name = format!(
                    "daydoc-{}.tar.gz",
                    chrono::Local::now().format("%Y-%m-%d")
                );
                println!("{} backup(s) in {}", existing.len(), dir.display());
                println!("would write {}", dir.join(name).display());
                return Ok(ExitCode::SUCCESS);
            }
            let report = backup::run(&vault, &dir)?;
            print!("{}", report.render());
            Ok(ExitCode::SUCCESS)
        }

        Sub::Check(_) => {
            let vault = open_vault(explicit, read_only)?;
            let changes = index::check_identity(&vault)?;
            let purged = vault.purge_trash()?;
            if changes.is_empty() {
                println!("note identities stable");
            } else {
                for change in &changes {
                    println!("{}", change.describe());
                }
                println!("{} warning(s) written to _meta/warnings/", changes.len());
            }
            println!("purged {purged} trash file(s)");
            Ok(ExitCode::SUCCESS)
        }

        Sub::Import(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let source = PathBuf::from(&cmd.path);
            let report = if cmd.dry_run {
                migrate::preview(&vault, &source)?
            } else {
                migrate::run(&vault, &source)?
            };
            if cmd.json {
                print_json(&report);
                return Ok(ExitCode::SUCCESS);
            }
            print!("{}", report.render());
            Ok(ExitCode::SUCCESS)
        }

        Sub::Cluster(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let proposals = cluster::propose(&vault)?;
            if cmd.json {
                print_json(&proposals);
                return Ok(ExitCode::SUCCESS);
            }
            if proposals.is_empty() {
                println!(
                    "no project clusters: {} or more notes within {} days must share a client domain or a person",
                    cluster::MIN_NOTES,
                    cluster::WINDOW_DAYS
                );
                return Ok(ExitCode::SUCCESS);
            }
            if !cmd.accept {
                for (i, proposal) in proposals.iter().enumerate() {
                    println!("{}. {}", i + 1, proposal.render());
                }
                println!("accept one with: daydoc cluster --accept --index 1");
                return Ok(ExitCode::SUCCESS);
            }
            let index = cmd.index.unwrap_or(1);
            if index == 0 || index > proposals.len() {
                return Err(DayDocError::Config(format!(
                    "proposal {index} is out of range (1..{})",
                    proposals.len()
                )));
            }
            let note = cluster::accept(&vault, &proposals[index - 1])?;
            println!("wrote {}", note.rel_path);
            Ok(ExitCode::SUCCESS)
        }

        Sub::Attach(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let source = PathBuf::from(&cmd.file);
            match cmd.note {
                Some(note_rel) => {
                    let (attachment, _) = attach::attach_to(&vault, &note_rel, &source)?;
                    println!("{} linked from {}", attachment.rel_path, note_rel);
                }
                None => {
                    let attachment = attach::store(&vault, &source)?;
                    let note = if attachment.deduplicated {
                        " (already present)"
                    } else {
                        ""
                    };
                    println!("{}  {} bytes{}", attachment.rel_path, attachment.bytes, note);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Purge(_) => {
            let vault = open_vault(explicit, read_only)?;
            let removed = vault.purge_trash()?;
            println!(
                "purged {removed} file(s) past the {} day retention window",
                vault.config().trash_retention_days
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Replay(_) => {
            let vault = open_vault(explicit, read_only)?;
            let warnings = vault.replay_wal()?;
            if warnings.is_empty() {
                println!("nothing staged");
            }
            for warning in warnings {
                println!("{warning}");
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Status(cmd) => {
            let vault = open_vault(explicit, read_only)?;
            let scan = vault.scan()?;
            let cards = ledger::cards(&vault)?;
            let flags = stale::scan(&vault)?;
            if cmd.json {
                let summary = serde_json::json!({
                    "vault": vault.root().display().to_string(),
                    "notes": scan.notes.len(),
                    "broken": scan.broken.len(),
                    "decisions": cards.len(),
                    "proposed": cards.iter().filter(|c| c.status == DecisionStatus::Proposed).count(),
                    "accepted": cards.iter().filter(|c| c.status == DecisionStatus::Accepted).count(),
                    "stale": flags.len(),
                    "llm_mode": vault.config().llm_mode,
                    "read_only": vault.is_read_only(),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).unwrap_or_default()
                );
                return Ok(ExitCode::SUCCESS);
            }
            println!("vault   {}", vault.root().display());
            println!("notes   {}", scan.notes.len());
            if !scan.broken.is_empty() {
                println!("broken  {}", scan.broken.len());
            }
            println!(
                "decisions {} ({} proposed, {} accepted)",
                cards.len(),
                cards
                    .iter()
                    .filter(|c| c.status == DecisionStatus::Proposed)
                    .count(),
                cards
                    .iter()
                    .filter(|c| c.status == DecisionStatus::Accepted)
                    .count()
            );
            println!("stale   {}", flags.len());
            println!("llm     {} (default off)", vault.config().llm_mode);
            Ok(ExitCode::SUCCESS)
        }
    }
}
