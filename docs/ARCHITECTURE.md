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
| `types` | Note types and the decision lifecycle, including the allowed transitions |
| `ids` | The two ID spaces: ULID for linking, `DR-NNNN` for humans, and slugs |
| `ledger` | Counter allocation, create, supersede, revert, transitions, cards |
| `classify` | Nine-type intent classifier: cue table + structural signals |
| `capture` | Classify → route, or inbox with a badge |
| `meeting` | Sentence split, cue scoring, owner NER, candidate cards, approval |
| `sop` | Ordered trigram signatures, union-find clustering, proposals |
| `stale` | Age by type + contradiction (superseded decision, renamed project, departed owner) |
| `search` | Fuzzy token match with edit distance, filters, decision cards, `ask` |
| `index` | SQLite metadata + FTS5 candidates, file-count stamp, note identity guard |
| `embed` | The `Embedder` trait, a deterministic feature-hash embedder, cosine, vector store |
| `link` | Auto-link suggestions from a shared entity or similarity; accept writes links |
| `summarize` | TextRank extractive summary |
| `handover` | Period packet: did, open, decided, who to ask, stale |
| `daily` | Daily note, EOD wrap, ISO-week digest, the 17:00 prompt |
| `migrate` | Import an existing Markdown folder, dry-run first, symlinks skipped |
| `cluster` | Project proposals from a shared client domain or person |
| `attach` | Content-addressed attachments, deduplicated by hash |
| `calendar` | Read-only `.ics` signal: event titles bias project tagging |
| `lint` | Vault health: ids, types, citations, links, attachments; `--fix` for ids only |
| `stats` | Vault counts, entities and date range |
| `export` | One self-contained HTML file for a period |
| `backup` | Dated `.tar.gz`, 7/4/6 retention, AES-256-GCM, restore with `--force` |
| `crypto` | AES-256-GCM with a `DAYDOC1` header; wrong key and tampering fail closed |
| `secret` | `KeyStore` trait: environment, OS keychain, `0600` file |
| `sync` | Git commit-only through `git2`; conflict sidecars; never holds credentials |
| `watch` | Filesystem watcher, debounce, per-path reindex, timed commit |
| `llm` | Optional BYOK chat: retrieval, preview, provider request shapes |
| `mobile` | Capture-only (and optionally read) HTTP surface with two tokens |
| `ui` | The egui shell: capture, ledger, search, stale, status, Indic fonts, hotkeys |

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
| Encrypted backup with no key | Nothing is written; the error names the command to fix it |
| Wrong or missing key on restore | Decryption fails closed; no partial files are created |

## Deliberately absent

No WASM plugin host, no multi-instance coordination, no file-type detection
pipeline, no quarantine daemon, no rate limiter, no telemetry. Each one is a
source of failure modes without serving the ledger.
