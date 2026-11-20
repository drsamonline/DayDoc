# DayDoc

**Decision-first office notes.** Every notes app stores notes. DayDoc stores
decisions — and treats notes as the evidence for them.

A note is one of four things:

| Kind | What it is |
| --- | --- |
| **Decision** | An atomic object: `DR-NNNN`, a lifecycle, a supersession chain. |
| **Evidence** | A note that supports, cites or contradicts a decision. |
| **Procedure** | An SOP that emerged from the same steps being written down repeatedly. |
| **Task** | Something derived from a decision or a meeting. |

That ontology is the product. Not the editor, not the sync, not an AI feature
bolted onto a text box.

## What DayDoc is not

- Not a file manager or sorter.
- Not a team wiki. It is a personal vault; sharing is git host access control.
- Not a notes app with AI added on. The ledger is the schema.
- Not a cloud service: no account, no server, no telemetry.

## Identity

Rust, Windows 10/11 and Linux x86_64 (no macOS). The vault is plain Markdown
plus YAML on disk. It works offline forever, and uninstalling DayDoc loses
nothing.

## Build and run

```bash
cargo build --release
cargo test
```

```bash
daydoc init ~/vault                 # three clicks, no wizard
daydoc capture "we decided to adopt Okta for SSO"   # -> decisions/DR-0001.md
daydoc capture --text "- [ ] send the invoice"      # text starting with a dash
cat note.md | daydoc capture -                       # or from stdin
daydoc decisions                    # the ledger, as cards
daydoc ask "what did we decide about okta"
```

## The decision ledger

Two ID spaces, deliberately separate:

- `id` — a ULID. Internal linking, stable across renames and moves.
- `decision_id` — `DR-NNNN`. The human reference **and** the supersession
  pointer.

Lifecycle: `proposed` → `accepted` → `superseded`, or `proposed` → `rejected`.

Supersession is explicit and **revertible**:

```bash
daydoc supersede DR-0003 "Use Entra ID for SSO" --decision "Entra is cheaper at our volume"
# DR-0003 -> status: superseded, superseded_by: DR-0004, superseded_at: today
daydoc revert DR-0004 --reason "wrong call, Okta stays"
# both accepted again, both links cleared, change audited
```

Reversing an accepted decision is always a **new** decision that supersedes it.
History is never edited.

## What Tier 0 does, with no model and no network

- **Capture with intent classification.** One box. Nine note types, a weighted
  cue table plus structural signals. Below 0.7 confidence the note lands in
  `inbox/` with a type badge instead of being misfiled.
- **Meeting → decision.** Paste a transcript; candidate decisions come back as
  one-line cards (`Adopt Okta for SSO — approve as DR-0012?`) with the owner
  pre-filled. Nothing is written until you approve.
- **SOP extraction.** Three or more notes sharing the same *ordered* sequence
  of steps propose a draft SOP. Ordered n-grams, not bag-of-words.
- **Stale detection, two independent signals.** Age by type (configurable) and
  contradiction (cites a superseded decision, a renamed project, a departed
  owner). Three ways out: re-verify, edit, archive.
- **Search that returns cards.** `ask` gives you the decision, its status, its
  owner, what it replaced, what it is cited by — not a paragraph to re-read.
- **Handover packet, wrap and weekly digest.** Markdown only.
- **Import with a dry-run preview.** Point it at an Obsidian, Logseq or plain
  folder: paths are preserved, missing ids are assigned, missing types are
  classified, nothing is overwritten.
- **Project auto-clustering.** Five or more notes in seven days sharing a client
  domain or a person propose a project folder. Accepting files the members.
- **Backups with retention.** A dated `.tar.gz` of the vault, last 7 daily,
  4 weekly, 6 monthly. Backups live in the state directory, not inside the
  vault they protect. `backup_encrypt = true` writes `.tar.gz.enc`
  (AES-256-GCM) and refuses to write anything at all without a key.
- **Restore.** `daydoc restore <file> --into <dir>`, decrypting when needed and
  refusing archive entries that would escape the target directory.
- **Keys.** `daydoc key set backup --generate` prints a key once and stores it in
  the OS keychain, falling back to a `0600` file in the state directory on a
  machine with no usable keychain; `key show` says which store holds it and
  `DAYDOC_BACKUP_KEY` overrides both. Values are never printed.
- **Identity guard.** Change a note's `id` and the file is a new note: the old
  id stays in the index and a warning lands in `_meta/warnings/`.
- **`--json` on `decisions`, `search`, `stale`, `status` and `lint`.**
- **A watcher for external edits.** Edit the vault in VS Code or Obsidian and
  `daydoc watch` reindexes on a debounce and commits on a timer; our own
  bookkeeping and `.git` never re-trigger it.
- **An index that heals itself.** Metadata and a SQLite FTS5 table live outside
  the vault; on open, a disagreeing index is rebuilt. Search uses it when it can
  match and falls back to a fuzzy scan when a query is a typo.
- **Git sync, commit only.** Stages and commits the vault; pushing is your own
  credential helper's job, so DayDoc never holds a token. A merge conflict
  becomes a `.conflict.md` sidecar with both sides, never an automatic merge.
- **Export a period.** One self-contained HTML file — no scripts, no external
  requests, note bodies escaped and shown as text.
- **Config without an editor.** `daydoc config set stale.sop 30`.
- **Lint, with one safe fix.** `lint --fix` assigns a ULID to notes that have
  none and nothing else; duplicate ids, unknown citations and orphaned
  attachments stay reported, because they are judgement calls.
- **Lint.** Missing and duplicate ids, unknown types, citations of decisions
  that do not exist, broken `links:`, missing and orphaned attachments.

An optional LLM (BYOK or a local server) adds chat over the ledger. It is off by
default, there is no download prompt, and no feature above needs it. When it is
on, three things hold:

- retrieval sends the top-k notes only, never the vault, capped at 8,000
  characters;
- `llm_send_content = false` refuses to send anything, whatever else is set;
- `daydoc chat --preview` prints the exact system prompt and note text that would
  leave the machine, and sends nothing.

The key comes from `daydoc key set llm` (or `DAYDOC_LLM_KEY`), never from the
config file and never logged. The OS keychain is the next store behind the same
trait; until then the key file is `0600` and the threat model is written down in
`src/secret.rs`.

## Guarantees

- **Writes are staged then committed.** Every write goes to a write-ahead log in
  the state directory, is fsync'd, then committed by rename. A crash replays on
  start; a corrupt log entry is moved aside and the vault is left untouched.
- **Nothing outside the vault is addressable.** Relative paths only: no `..`, no
  absolute paths, no symlink following on delete.
- **Broken frontmatter never destroys a file.** The file is left as it is,
  copied to `_meta/broken/`, and reported.
- **The index is disposable.** Counters and caches live outside the vault; the
  vault is the source of truth.

## Commands

```
init        create a vault
capture     classify and store one capture
meeting     extract decision candidates from a transcript
decisions   list the ledger as cards
show        one decision with its chain
accept      accept a proposed decision
reject      reject a proposed decision
supersede   record a decision that replaces an older one
revert      undo a supersession
search      fuzzy search with filters
ask         time-anchored answer as a card
stale       every stale or contradicting note
verify      re-verify a note
archive     move a note to _archive/YYYY/
ui          open the desktop window
chat        ask the optional llm (off by default, --preview shows the payload)
stats       vault counts, entities and dates
export      write a period as one self-contained html file
config      show or change daydoc.toml
sync        commit the vault to git (never pushes)
lint        report vault problems: ids, links, citations, assets
reindex     rebuild the search index from the vault
watch       reindex on external edits, commit on a timer
backup      write a dated .tar.gz (optionally encrypted) and apply retention
restore     unpack a backup into a folder
key         store, show or clear a key (values never printed)
check       identity check plus trash purge
import      import an existing Markdown folder (dry-run preview)
cluster     propose project folders from notes that cluster
attach      store a file content-addressed under assets/
purge       delete trash past the retention window
handover    compile a handover packet (--json for the facts)
wrap        end-of-day wrap
digest      weekly digest
sop         propose or accept an SOP
replay      replay staged writes after a crash
status      vault counts and open threads
```

## Licence

Not chosen yet. Add a `LICENSE` file and the matching `license` field in
`Cargo.toml` before publishing binaries.

## The window

`daydoc ui` opens the shell: capture with a live classification preview,
the ledger as cards, search, stale notes with a re-verify button, and vault
statistics. It is a view over the same library as the CLI, so nothing exists in
the window that is not in the vault.

Two limits, stated plainly: the bundled fonts cover Latin script only, so Indic
text needs a font added before it renders correctly; and the shell has no tray
icon or global hotkey yet — those need platform code beyond the window itself.

## Packaging

`v*` tags build Linux and Windows archives plus a `.deb`. See
`docs/PACKAGING.md` for what ships and what does not yet.

## Status

Tier 0 core is implemented and tested: vault, ledger, supersession and revert,
classification, meeting extraction, SOP proposals, staleness, search, handover,
wrap, digest, CLI. Not yet built: the egui desktop shell, tray, bundled
embeddings (MiniLM / bert-tiny-NER via `rten`), auto-linking, project
auto-clustering, attachments, git sync, mobile PWA, encrypted backup, packaging
and self-update. See `docs/ROADMAP.md`.

## Layout

```
src/
  llm.rs          optional BYOK chat: retrieval, preview, provider requests
  secret.rs       key storage: environment, 0600 file, keychain-ready trait
  crypto.rs       AES-256-GCM for encrypted backups
  watch.rs        filesystem watcher, debounce, reindex and timed commit
  index.rs        SQLite metadata + FTS5 index, note identity guard
  lint.rs         vault health checks
  stats.rs        vault counts and entities
  export.rs       one-file HTML export
  migrate.rs      import an existing Markdown folder
  cluster.rs      project auto-clustering (Tier 0)
  attach.rs       content-addressed attachments
  vault.rs        write-ahead log, atomic commit, path safety, trash, audit
  frontmatter.rs  YAML read/write, unknown keys preserved
  note.rs         note model, section helpers
  ledger.rs       decision ledger: allocate, create, supersede, revert
  classify.rs     intent classifier (Tier 0)
  meeting.rs      transcript -> decision candidates
  sop.rs          ordered n-gram procedure matching
  stale.rs        age + contradiction signals
  search.rs       fuzzy search, decision cards, time-anchored answers
  summarize.rs    TextRank extractive summary
  handover.rs     handover packet
  capture.rs      capture routing
  daily.rs        daily note, wrap, digest
  main.rs         CLI
```
