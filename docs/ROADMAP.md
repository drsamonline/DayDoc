# Roadmap

Ordered by what makes the ledger trustworthy, then what makes it pleasant.

## Done (Tier 0 core)

- Vault: WAL + atomic commit, path safety, scan, trash, archive, audit log
- Frontmatter: tolerant read, deterministic write, unknown keys preserved
- Decision ledger: `DR-NNNN` allocation, lifecycle, supersession, revert
- Classification: nine note types, confidence, inbox routing
- Meeting pipeline: candidate extraction, owner pre-fill, approve-to-record
- SOP proposals: ordered n-gram matching, union-find clustering
- Staleness: age by type + contradiction, three resolutions
- Search: fuzzy, filtered, decision cards, time-anchored `ask`
- SQLite FTS5 index with self-healing rebuild; search falls back to fuzzy scan
- Git sync: commit-only through `git2`, `.conflict.md` sidecars, no credentials
- Stats, config show/set, single-file HTML export
- Release workflow: Linux and Windows archives plus a `.deb` on `v*` tags
- OS keychain via `keyring`, with the `0600` file as fallback
- `lint --fix` for notes with no id
- Encrypted backups (AES-256-GCM), restore, and a key store with a `0600` file
- Optional LLM: top-k retrieval, send-content lock, `--preview`, provider request
  shapes for OpenAI-compatible, Anthropic and Gemini
- Desktop shell: capture, decisions, search, stale, status over the library
- End-of-day wrap offered by the shell after 17:00 local
- Ten Indic scripts bundled (Lohit, SIL OFL 1.1)
- `--json` on meeting, ask, import, sop and cluster, completing the surface
- Global shortcuts: `Ctrl+Shift+Space` capture, `Ctrl+Shift+K` search
- Watcher: debounced reindex plus timed commit for external edits
- Structured `--json` for handover, wrap and digest
- Lint: ids, types, citations, links, attachments
- Backup: dated `.tar.gz` of the vault, 7/4/6 retention, fail-closed on encryption
- Identity guard: changed `id` is a new note, warned into `_meta/warnings/`
- `--json` output for decisions, search, stale and status
- Import of an existing Markdown folder, with dry-run preview
- Project auto-clustering from shared client domain or person
- Content-addressed attachments under `assets/`
- TextRank summaries, handover packet, EOD wrap, weekly digest
- CLI over the whole surface, end-to-end tests

## Next

1. **Embeddings.** The pipeline, the vector store and auto-linking exist behind
   `embed::Embedder`. What is missing is the model: bundle all-MiniLM-L6-v2 INT4
   (~11 MB) and bert-tiny-NER INT4 (~2.2 MB) through `rten`, zstd-compressed and
   decompressed at load, then move the cosine scan to `sqlite-vec`.
2. **LLM follow-ups.** Key in the OS keychain (`keyring`), a chat panel in the
   shell, and write actions behind an Approve card.
3. **Shell follow-ups.** A tray icon (`tray-icon` needs Rust 1.90 and
   libappindicator headers on Linux), `Ctrl+Shift+M` for the morning plan, a
   native Monday-digest notification, and the floating capture overlay that sits
   above other windows.
3. **Keychain edge cases.** The keychain store exists; what is left is a
   Windows credential-manager test pass and a locked-session error message.
4. **Mobile follow-ups.** Capture and read are both in place behind separate
   tokens; what is left is a service worker for a true offline install.
5. **Packaging.** `cargo-wix` MSI plus portable `.exe`; AppImage plus `.deb`;
   `self_update` from GitHub Releases.

## Before 1.0

See `docs/AUDIT.md`. Licence, `SECURITY.md` and the `restore --force` guard are
done; what remains is one human pass over the window.

## Explicitly out of scope

macOS, team accounts, a hosted service, telemetry, WASM plugins, multi-instance
coordination.
