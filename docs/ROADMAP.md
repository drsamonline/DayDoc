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
- Backup: dated `.tar.gz` of the vault, 7/4/6 retention, fail-closed on encryption
- Identity guard: changed `id` is a new note, warned into `_meta/warnings/`
- `--json` output for decisions, search, stale and status
- Import of an existing Markdown folder, with dry-run preview
- Project auto-clustering from shared client domain or person
- Content-addressed attachments under `assets/`
- TextRank summaries, handover packet, EOD wrap, weekly digest
- CLI over the whole surface, end-to-end tests

## Next

1. **Index and embeddings.** Move search onto a SQLite FTS + `sqlite-vec`
   index; bundle all-MiniLM-L6-v2 INT4 (~11 MB) and bert-tiny-NER INT4
   (~2.2 MB) through `rten`, zstd-compressed, decompressed at load. Auto-link
   suggestions (never auto-accepted) and fuzzy SOP step matching ride on it.
2. **Desktop shell.** egui: capture overlay on `Ctrl+Shift+Space`, search on
   `Ctrl+Shift+K`, morning plan on `Ctrl+Shift+M`, tray badge for stale notes
   and the re-verify entry point.
3. **Watcher.** `notify-debouncer-full` re-index on external edits (VS Code,
   Obsidian), with the Windows lock retry already in `vault`.
4. **Git sync.** `git2`, batched auto-commit every 300 s, commit only, push
   through the user's own credential helper, `.conflict.md` sidecars, never
   auto-merged.
5. **Backup encryption.** AES-256-GCM with the key in the OS keychain;
   `backup_encrypt = true` refuses to run until this lands.
6. **Mobile PWA.** `axum`, capture-only, LAN + token auth, offline queue.
7. **Packaging.** `cargo-wix` MSI plus portable `.exe`; AppImage plus `.deb`;
   `self_update` from GitHub Releases.

## Explicitly out of scope

macOS, team accounts, a hosted service, telemetry, WASM plugins, multi-instance
coordination.
