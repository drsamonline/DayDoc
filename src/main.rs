//! `daydoc` — the command line surface. The GUI calls the same library.

use std::path::PathBuf;
use std::process::ExitCode;

use argh::FromArgs;

use daydoc::error::DayDocError;
use daydoc::types::{DecisionStatus, NoteType};
use daydoc::{capture, daily, handover, ledger, meeting, search, sop, stale, Vault};

#[derive(FromArgs, Debug)]
/// decision-first office notes (DayDoc)
struct TopLevel {
    #[argh(option, short = 'v')]
    /// vault root (default: $DAYDOC_VAULT, then the current directory)
    vault: Option<String>,
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
    text: String,
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
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "stale")]
/// show every note that is stale or contradicts the ledger
struct StaleCmd {}

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
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "sop")]
/// propose SOPs from repeated procedures, or accept one
struct SopCmd {
    #[argh(switch)]
    /// accept a proposal instead of listing
    accept: bool,
    #[argh(option)]
    /// proposal number to accept, 1-based
    index: Option<usize>,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "replay")]
/// replay staged writes left behind by a crash
struct ReplayCmd {}

#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "status")]
/// vault counts and open threads
struct StatusCmd {}

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

fn open_vault(explicit: Option<String>) -> daydoc::Result<Vault> {
    let root = explicit
        .or_else(|| std::env::var("DAYDOC_VAULT").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let vault = Vault::open(&root)?;
    for warning in vault.replay_wal()? {
        eprintln!("daydoc: {warning}");
    }
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

fn today_or(raw: Option<String>) -> String {
    raw.unwrap_or_else(daydoc::vault::today)
}

fn run(args: TopLevel) -> daydoc::Result<ExitCode> {
    let explicit = args.vault.clone();
    match args.cmd {
        Sub::Init(cmd) => {
            let vault = Vault::init(&PathBuf::from(&cmd.path))?;
            println!("vault ready at {}", vault.root().display());
            println!("  {} note(s) already here", vault.note_count()?);
            Ok(ExitCode::SUCCESS)
        }

        Sub::Capture(cmd) => {
            let vault = open_vault(explicit)?;
            let text = read_text(&cmd.text)?;
            if cmd.preview {
                let classification = capture::preview(&text);
                println!(
                    "would route as {} at {:.0}% (confident: {})",
                    classification.note_type,
                    classification.confidence * 100.0,
                    classification.is_confident(vault.config().min_classification_confidence)
                );
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
            let vault = open_vault(explicit)?;
            let raw = read_text(&cmd.file)?;
            // A meeting note keeps its frontmatter; a raw transcript has none.
            let text = match daydoc::frontmatter::split(&raw) {
                Ok((_, body)) => body,
                Err(_) => raw,
            };
            let candidates = meeting::extract_decisions(&text);
            let actions = meeting::extract_action_items(&text);
            let next = meeting::next_reference(&vault);

            println!("{} candidate(s) in {}", candidates.len(), cmd.file);
            for (i, candidate) in candidates.iter().enumerate() {
                println!("  {}. {}", i + 1, candidate.card(&next));
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
                println!("\nrun `daydoc meeting {} --approve 1,2` to record these", cmd.file);
                return Ok(ExitCode::SUCCESS);
            }
            let written = meeting::approve(&vault, &cmd.file, &chosen)?;
            for note in &written {
                println!(
                    "recorded {} — {}",
                    note.frontmatter.decision_id.as_deref().unwrap_or("?"),
                    note.title()
                );
            }
            if cmd.actions {
                let added = meeting::append_action_items(&vault, &cmd.file, &actions)?;
                println!("wrote {added} action item(s) into {}", cmd.file);
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Decisions(cmd) => {
            let vault = open_vault(explicit)?;
            let wanted = match cmd.status.as_deref() {
                Some(raw) => Some(DecisionStatus::parse(raw).ok_or_else(|| {
                    DayDocError::Config(format!("unknown status \"{raw}\""))
                })?),
                None => None,
            };
            let mut shown = 0;
            for card in ledger::cards(&vault)? {
                if let Some(wanted) = wanted {
                    if card.status != wanted {
                        continue;
                    }
                }
                print!("{}", card.render());
                println!();
                shown += 1;
            }
            if shown == 0 {
                println!("no decisions yet — `daydoc capture \"we decided to ...\"`");
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Show(cmd) => {
            let vault = open_vault(explicit)?;
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
            let vault = open_vault(explicit)?;
            let note = ledger::accept(&vault, &cmd.reference)?;
            println!(
                "{} is now {}",
                note.frontmatter.decision_id.as_deref().unwrap_or("?"),
                ledger::status_of(&note)
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Reject(cmd) => {
            let vault = open_vault(explicit)?;
            let note = ledger::reject(&vault, &cmd.reference)?;
            println!(
                "{} is now {}",
                note.frontmatter.decision_id.as_deref().unwrap_or("?"),
                ledger::status_of(&note)
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Supersede(cmd) => {
            let vault = open_vault(explicit)?;
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
            let vault = open_vault(explicit)?;
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
            let vault = open_vault(explicit)?;
            let mut filters = search::Filters::default();
            if let Some(kind) = cmd.kind.as_deref() {
                filters.note_type = Some(parse_kind(kind)?);
            }
            filters.person = cmd.person;
            filters.project = cmd.project;
            filters.status = cmd.status;
            let hits = search::search(&vault, &cmd.query, &filters)?;
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
            let vault = open_vault(explicit)?;
            let cards = search::ask(&vault, &cmd.question)?;
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

        Sub::Stale(_) => {
            let vault = open_vault(explicit)?;
            let flags = stale::scan(&vault)?;
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
            let vault = open_vault(explicit)?;
            let note = stale::reverify(&vault, &cmd.path)?;
            println!(
                "re-verified {} ({})",
                note.rel_path,
                note.frontmatter.last_verified.as_deref().unwrap_or("today")
            );
            Ok(ExitCode::SUCCESS)
        }

        Sub::Archive(cmd) => {
            let vault = open_vault(explicit)?;
            let dest = stale::archive(&vault, &cmd.path)?;
            println!("archived to {dest}");
            Ok(ExitCode::SUCCESS)
        }

        Sub::Handover(cmd) => {
            let vault = open_vault(explicit)?;
            let to = today_or(cmd.to);
            let from = cmd.from.unwrap_or_else(|| {
                let date = chrono::Local::now().date_naive() - chrono::Duration::days(14);
                date.format("%Y-%m-%d").to_string()
            });
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
            let vault = open_vault(explicit)?;
            let date = today_or(cmd.date);
            if cmd.dry_run {
                print!("{}", daily::eod_wrap(&vault, &date)?);
            } else {
                let rel = daily::write_wrap(&vault, &date)?;
                println!("wrap written into {rel}");
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Digest(cmd) => {
            let vault = open_vault(explicit)?;
            let date = today_or(cmd.date);
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
            let vault = open_vault(explicit)?;
            let proposals = sop::propose(&vault)?;
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

        Sub::Replay(_) => {
            let vault = open_vault(explicit)?;
            let warnings = vault.replay_wal()?;
            if warnings.is_empty() {
                println!("nothing staged");
            }
            for warning in warnings {
                println!("{warning}");
            }
            Ok(ExitCode::SUCCESS)
        }

        Sub::Status(_) => {
            let vault = open_vault(explicit)?;
            let scan = vault.scan()?;
            let cards = ledger::cards(&vault)?;
            let flags = stale::scan(&vault)?;
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
