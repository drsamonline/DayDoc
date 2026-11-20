# Working in this repository

DayDoc is a Rust desktop app: a decision ledger over plain Markdown. Read
`README.md` first, then `docs/ONTOLOGY.md` for the model and `docs/AUDIT.md` for
what is known to be weak.

## Verify before you claim

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --no-default-features --lib     # the core must build without ui/git/mobile
bash scripts/demo.sh /tmp/daydoc-demo      # the documented walkthrough
```

CI runs all of that on Linux and Windows, plus `cargo deb`. A change that is not
covered by a command whose output you have read is not done.

## The rules that are not style

1. Every vault write goes through `Vault::write` — the WAL, the checksum and the
   atomic rename are the crash story. A stray `fs::write` into the vault breaks
   it.
2. Vault paths are relative only. `Vault::abs` rejects `..`, absolute paths and
   prefixes; do not canonicalise your way around it. When you accept a path from
   a user, resolve it to a vault-relative one (`resolve_source` in `main.rs`).
3. History is appended. Never edit a decision's body to reflect a reversal:
   supersede it, or revert the supersession.
4. Tier 0 first. Every feature must work with no model, no network and no
   download. `llm_mode = "off"` is the default and the tested path.
5. Nothing phones home. No telemetry, no analytics, no update ping.
6. A secret never lands in the vault, in `daydoc.toml`, in the audit log, in a
   preview, or in a request body. Keys go in headers; `key show` prints where a
   key lives, never what it is.
7. Broken input never destroys a file. Broken YAML is copied aside and reported.
8. Model-facing code sits behind `embed::Embedder`. Do not reach around it.

## Shape of a change

- New note type: `types::NoteType` (variant, `as_str`, `parse`, `folder`,
  `stale_key`), then `classify::cues`, then a `config::StaleThresholds` key if it
  can go stale, then tests in both.
- New command: a struct in `main.rs` with a doc comment on **every** field — argh
  refuses to compile otherwise, and the description must start lowercase. Add it
  to the README command list; `tests/doc_drift.rs` checks that you did.
- New module: `src/<name>.rs`, registered in `src/lib.rs`, listed in the module
  map in `docs/ARCHITECTURE.md`; `tests/doc_drift.rs` checks that too.

## Where things are written down

| Question | File |
| --- | --- |
| What is this product | `README.md` |
| What is a decision, an SOP, a task | `docs/ONTOLOGY.md` |
| How the write path and modules work | `docs/ARCHITECTURE.md` |
| What is missing and in what order | `docs/ROADMAP.md` |
| What is known to be wrong | `docs/AUDIT.md` |
| Why a choice was made | `docs/DECISIONS.md` |
| Packaging and releases | `docs/PACKAGING.md` |
| What changed | `CHANGELOG.md` |
