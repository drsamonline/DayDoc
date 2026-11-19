# Architecture

## Invariants

1. **The vault is the source of truth.** Plain `.md` files with YAML
   frontmatter. Delete every cache and index and nothing is lost; delete DayDoc
   and the notes still open in any editor.
2. **History is appended, never rewritten.** A decision's body is never edited
   to reflect a reversal. A new decision supersedes it, and both stay readable.
3. **Two ID spaces.** `id` (ULID) links and survives renames. `decision_id`
   (`DR-NNNN`) is the human reference and the supersession pointer. A file path
   is a mutable index field, never an identity.
4. **No silent writes.** Every mutation goes through `Vault::write`, which
   stages in a write-ahead log, fsyncs, then commits by rename.
5. **Tier 0 first.** Classification, extraction, staleness and search all work
   with no model, no network, no download. Models and LLMs are additive.

## Write path

```
Vault::write(rel, content)
  ├─ ensure_writable(rel)              read-only vault refuses
  ├─ abs(rel)                          reject absolute paths, `..`, prefix
  ├─ stage  <state>/wal/<ULID>.wal     header {target, len, blake3} + body, fsync
  ├─ commit <target> via .daydoc-tmp-<ULID> + rename (Windows 32/33 backoff 1→16s)
  └─ unlink the WAL entry

startup: Vault::replay_wal()
  ├─ length + blake3 match  -> commit, delete entry
  └─ mismatch or bad header -> move to <state>/wal/rejected/, vault untouched
```

The WAL lives in the state directory (`$DAYDOC_HOME` or `~/.daydoc`), not in the
vault, so a synced vault never carries half-finished writes.

## Module map

| Module | Responsibility |
| --- | --- |
| `vault` | WAL, atomic commit, path safety, scan, trash, archive, audit log, SQLite counters |
| `frontmatter` | YAML in/out; unknown keys preserved so a round trip loses nothing |
| `note` | Note model, `## Section` helpers, canonical decision body |
| `ledger` | Counter allocation, create, supersede, revert, transitions, cards |
| `classify` | Nine-type intent classifier: cue table + structural signals |
| `meeting` | Sentence split, cue scoring, owner NER, candidate cards, approval |
| `sop` | Ordered trigram signatures, union-find clustering, proposals |
| `stale` | Age by type + contradiction (superseded decision, renamed project, departed owner) |
| `search` | Fuzzy token match with edit distance, filters, decision cards, `ask` |
| `summarize` | TextRank extractive summary |
| `handover` | Period packet: did, open, decided, who to ask, stale |
| `capture` | Classify → route, or inbox with a badge |
| `daily` | Daily note, EOD wrap, ISO-week digest |

## Counters

`DR-NNNN` comes from a SQLite `counters` table in the state directory, allocated
inside a `BEGIN IMMEDIATE` transaction so two writers cannot take the same
number. Deleting a note never frees its number: a reference must not be reused.

## Failure behaviour

| Failure | Behaviour |
| --- | --- |
| Crash mid-write | Replay on next start; entry verified by length and BLAKE3 |
| Corrupt WAL entry | Moved to `wal/rejected/`, replay aborted for that entry, vault untouched |
| Broken YAML | File untouched, copy in `_meta/broken/`, note reported in `status` |
| Read-only vault | Every write path returns `ReadOnly`, replay skipped |
| Windows file lock | Retry on os error 32/33 with 1→16 s backoff |
| Trash delete | Symlinks refused; canonical path must be inside the vault |

## Deliberately absent

No WASM plugin host, no multi-instance coordination, no file-type detection
pipeline, no quarantine daemon, no rate limiter, no telemetry. Each one is a
source of failure modes without serving the ledger.
