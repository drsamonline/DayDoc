# Audit — 2026-11-20

A release-gate review of the whole repository at `main` (30 commits, 158 unit and
27 end-to-end tests, green on Linux and Windows). Method: read the intent, map
the surface, then hunt in priority order — data loss, security, performance,
design — and prove each finding against the code before writing it down.

Scope: every module under `src/`, the CLI, the shell, the CI and release
workflows, and the docs. Not verified here: the rendered look of the window (no
display in this environment), Windows-specific runtime behaviour (the Windows
build compiles and its tests pass in CI, but nothing has been run on Windows by
a human), and the real network path of the optional LLM.

## Findings

Severity is about what happens to a user's notes, not about how hard the fix is.

| # | Severity | Finding | Evidence | Status |
|---|---|---|---|---|
| 1 | High | The key file was written with the process umask and tightened afterwards, leaving a window in which an API or backup key is world-readable. | `src/secret.rs:113` — `std::fs::write` then `restrict` | **Fixed** — `write_private` creates the file with `0600` from the first byte, and still tightens a pre-existing file |
| 2 | High | The LLM key lookup ignored the provider: an `ANTHROPIC_API_KEY` in the environment could be sent to `api.openai.com` if it was found first. | `src/llm.rs:274` — one list of variables for every provider | **Fixed** — `env_names_for(provider)`; `DAYDOC_LLM_KEY` remains the explicit override |
| 3 | Medium | `write_wrap` truncated the daily note at `## End of day`, deleting any section the user had written below it. | `src/daily.rs:218` — `body.truncate(pos)` | **Fixed** — replaces only up to the next `## ` heading; a test writes a section after the wrap and checks it survives |
| 4 | Medium | Quarantining a broken note named the copy with a fresh ULID, so every scan left another copy: `status` run ten times produced ten files. | `src/vault.rs:270` | **Fixed** — the copy is named by a hash of its content; a test scans three times and asserts one file |
| 5 | Medium | `trash_retention_days = 0` passed validation, which meant the next purge deleted the trash immediately after it was written. | `src/config.rs` validation | **Fixed** — floor of 1 day, enforced in `set_key` and `validate` |
| 6 | Medium | `restore` unpacks into any directory, including a live vault, with no confirmation. A restore of an old backup silently overwrites newer notes. | `src/backup.rs` — `restore()` | **Fixed** — a non-empty destination is refused unless `--force`; a test asserts the refusal leaves the folder untouched |
| 7 | Low | `import` followed symlinks in the source folder, so a link to a file outside it was read and copied into the vault. | `src/migrate.rs` — `collect()` used `entry.metadata()` | **Fixed** — `symlink_metadata`, and links are skipped; a test imports a folder containing one |
| 8 | Low | Every watcher batch reindexed the whole vault rather than the changed notes, so the watcher's cost grew with the vault instead of with the edit. | `src/watch.rs` — `handle_batch` | **Fixed** — `index::upsert_path` per changed path, `remove` for deletions, then one stamp; a test deletes a note and asserts it leaves the index |
| 9 | Low | `ledger::cards` scans the vault twice (citations, then the ledger) and `stats` scans four times. Measured cost is small today; it is the reason the index exists. | `src/ledger.rs`, `src/stats.rs` | **Accepted** — recorded, not fixed |
| 10 | Low | No `LICENSE`, and `Cargo.toml` carried no licence field. Anyone cloning could not tell what they may do with it. | repository root | **Fixed** — MIT in `LICENSE` and in `Cargo.toml`, with `NOTICE` for the bundled fonts |
| 11 | Low | No `SECURITY.md`. An app that stores keys and personal notes should say how to report a vulnerability and what it does not protect against. | repository root | **Fixed** — `SECURITY.md` with a threat model and out-of-scope list, plus `PRIVACY.md` |
| 12 | Low | CI never built the crate without its optional features, so the `ui`, `git` and `keychain` cfg paths could rot silently. | `.github/workflows/ci.yml` | **Fixed** — `cargo build --no-default-features` and a lib test run on Linux |
| 13 | Low | No `CHANGELOG`. The commit history was the only record of what changed. | repository root | **Fixed** — `CHANGELOG.md`, Keep a Changelog format |
| 14 | Low | `capture --preview --kind task` ignored `--kind` and previewed only the classifier's guess. | `src/main.rs` capture arm | **Fixed** — the preview names the forced type and the guess |

## What held up

Worth recording, because a review that only lists defects is not a review.

- **No path traversal in restore.** `tar`'s `unpack_in` refuses entries that would
  leave the destination; the finding above is about *where* it unpacks, not
  whether it can escape.
- **No SQL injection.** Every statement is parameterised; the one dynamic string
  is an FTS5 `MATCH` expression built from alphanumeric tokens, and a syntax
  error falls back to a scan rather than failing the command.
- **No secret in a log, a preview or a request body.** The key lives in a header;
  `chat --preview` prints the payload and nothing else; `key show` prints where a
  key lives, never what it is. A test asserts the body does not contain the key.
- **Writes are staged.** Every note write goes through the WAL, is checksummed and
  replayed on start; a corrupt entry is moved aside rather than applied. The
  audit found no path that writes a note directly.
- **Broken input never destroys a file.** Broken YAML leaves the note untouched,
  copies it aside and reports it; the fix for finding 4 made that copy idempotent.
- **The HTML export escapes.** A test writes `<script>alert(1)</script>` into a
  note and asserts the export contains `&lt;script&gt;` and no `<script>` tag.

## Verdict

**Ship the core; do not call it 1.0 yet.** The five fixed findings were the ones
that could hurt a user's data or leak a key, and they are closed with tests. The
open items are either product decisions (licence, changelog) or scale work that
does not bite at the vault sizes this build is aimed at.

Unblock conditions for a tagged release:

1. ~~`LICENSE` chosen and added~~ — MIT, `LICENSE` and `Cargo.toml`.
2. ~~`SECURITY.md` written~~ — with `PRIVACY.md` and `NOTICE`.
3. ~~Finding 6 closed~~ — `restore` refuses a non-empty destination without
   `--force`.
4. A human opens the window once and confirms it renders. **Still open.**

All findings are closed except 9, which is recorded as accepted with the index as
its remedy.
