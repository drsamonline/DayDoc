# Changelog

All notable changes to DayDoc. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- The morning plan: `Ctrl+Shift+M` opens a Plan view, and what you write goes into
  the `## Plan` section of today's daily note.

## [0.2.0] — 2026-11-20

Everything since 0.1.0: linking, the calendar signal, the phone surface, and a
second audit pass.

### Added

- `--json` on `capture`, `accept`, `reject`, `verify`, `reindex` and `check`, so
  every command a script would call can be parsed.
- `daydoc calendar` — a read-only `.ics` signal: notes on an event's day that
  share a meaningful word with its title, with accept writing the project tag.
- `daydoc mobile` — a capture-only page for a phone on the same network, bearer
  token, offline queue in `localStorage`, and read endpoints that refuse with a
  reason rather than pretending not to exist. A separate read token, enabled with
  `mobile_capture_only = false`, unlocks `/notes` and `/note`; the capture token
  cannot read and the read token cannot capture.
- `daydoc link` — auto-link suggestions from a shared person or project plus
  embedding similarity, with accept writing the link and carrying entities
  across. The embedding pipeline sits behind one trait; the bundled embedder is a
  deterministic feature hash, so the similarity is vocabulary rather than meaning
  until the model lands.

### Fixed

- Indexing and embedding wrote one autocommit per statement, which was a thousand
  fsyncs for a thousand notes; both now run in a single transaction.
- `calendar` sliced a date by byte index, so an `.ics` with non-ASCII in it
  panicked the parser. It now takes characters and skips anything that is not
  eight ASCII digits.
- `link` embedded every note once per comparison, which is quadratic; vectors are
  computed once per run.
- `ledger::cards` scanned the vault twice, and `stats` and `handover` scanned
  again on top; they now share one scan.
- The mobile capture endpoint answered 500 when the vault refused a write; it
  answers 403.
- A note with no date in its frontmatter was invisible to the calendar signal; it
  falls back to the file's modification time.
- The architecture map listed 13 of 26 modules; `tests/doc_drift.rs` now fails if
  a module or a command is undocumented.

### Documentation

- `AGENTS.md` — the verification commands and the rules that are not style.
- `docs/USAGE.md` — ten tasks with the commands that do them.
- `docs/AUDIT.md` — a second pass over the modules added since 0.1.0.
- `tests/perf.rs` — budgets on indexing and searching 500 notes.

## [0.1.0] — 2026-11-20

The first release: the decision ledger and everything needed to keep it honest,
with no model, no account and no network.

### Added

- **Vault.** Plain Markdown plus YAML on disk. Every write is staged in a
  write-ahead log, checksummed and committed by rename; a crash replays on start
  and a corrupt entry is set aside. Relative paths only, symlinks refused on
  delete, `_trash` with retention, `_archive/YYYY/`, append-only
  `_meta/audit.jsonl`.
- **Decision ledger.** `DR-NNNN` allocated in one immediate transaction and never
  reused; lifecycle `proposed → accepted → superseded` or `→ rejected`; explicit
  supersession writing both sides; revertible supersession with an audit entry.
- **Capture.** One box, nine note types, weighted cue classification with a
  confidence gate that routes low confidence to `inbox/` instead of guessing.
- **Meeting pipeline.** Transcript to candidate cards with owner pre-fill and
  supersession hints; nothing is written until a candidate is approved.
- **SOP proposals.** Ordered n-gram matching with union-find clustering; three or
  more notes describing the same sequence propose a draft.
- **Stale detection.** Age by type plus contradiction (a superseded decision, a
  renamed project, a departed owner), with re-verify, edit and archive.
- **Search.** Fuzzy scoring with filters, a SQLite FTS5 index that heals itself,
  and `ask` returning decision cards rather than prose.
- **Loop.** TextRank end-of-day wrap, weekly digest, handover packet, and a
  single-file HTML export.
- **Import.** Existing Obsidian, Logseq or plain folders, with a dry-run preview;
  paths and existing ids are preserved and nothing is overwritten.
- **Project clustering.** Five or more notes in seven days sharing a client domain
  or a person propose a project folder.
- **Attachments.** Content-addressed at `assets/<xx>/<blake3>.<ext>`, deduplicated.
- **Backups.** Dated `.tar.gz`, 7 daily / 4 weekly / 6 monthly retention, optional
  AES-256-GCM with a key from the keychain or a `0600` file, and `restore` that
  refuses a non-empty destination without `--force`.
- **Git sync.** Commit-only through `git2`, never holding credentials; conflict
  markers become a `.conflict.md` sidecar rather than an automatic merge.
- **Watcher.** Debounced reindex of changed notes plus a timed commit, ignoring its
  own bookkeeping so a commit cannot trigger the next one.
- **Optional LLM.** Off by default; top-k retrieval capped at 8,000 characters,
  `llm_send_content = false` refuses to send, `--preview` prints the exact payload,
  and the key is matched to its provider.
- **Desktop shell.** Capture with a live preview, the ledger as cards, search,
  stale notes, statistics, ten bundled Indic scripts (Lohit, SIL OFL 1.1),
  `Ctrl+Shift+Space` and `Ctrl+Shift+K`, and the end-of-day wrap offered after
  17:00.
- **CLI.** Every command above, plus `lint` (with `--fix` for missing ids),
  `stats`, `export`, `config show/set`, `key set/show/clear`, `check`, `reindex`,
  `status`, and `--json` on every command that returns facts.

### Fixed

- The key file is created `0600` rather than tightened after writing, closing a
  window in which a key was readable.
- An `ANTHROPIC_API_KEY` could be sent to `api.openai.com`; the environment
  variable is now matched to the configured provider.
- The end-of-day wrap truncated any section written below it, and summarised its
  own previous wrap, growing on every run.
- A broken note was quarantined once per scan rather than once per note.
- `trash_retention_days = 0` purged the trash immediately; there is now a floor.
- `restore` could overwrite a live vault without warning.
- `import` followed symlinks out of the source folder.
- The watcher rebuilt the whole index on every save instead of indexing the
  changed paths.

### Security

- `SECURITY.md` states what DayDoc protects and what it does not: no at-rest
  encryption by default, the `0600` key file protects against other accounts and
  not against you, and the optional model sees what you send it.

[0.2.0]: https://github.com/drsamonline/DayDoc/releases/tag/v0.2.0
[0.1.0]: https://github.com/drsamonline/DayDoc/releases/tag/v0.1.0
