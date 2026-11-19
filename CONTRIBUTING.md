# Contributing

## Before you push

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

CI runs the same on `ubuntu-latest` and `windows-latest`. Formatting and clippy
are advisory while the surface is moving; build and test are not.

## Rules that are not negotiable

These come from the product, not from taste:

1. **No write outside `Vault::write`.** It stages in the write-ahead log and
   commits atomically. A stray `fs::write` into the vault defeats crash
   recovery.
2. **No absolute or escaping paths.** `Vault::abs` rejects them; do not
   `canonicalize` your way around it.
3. **History is appended.** Never edit a decision's body to reflect a reversal.
   Supersede it, or revert the supersession.
4. **Tier 0 first.** Any feature that needs a model or a network call must also
   work, degraded but honest, with none. `llm_mode = "off"` is the default and
   the tested path.
5. **Nothing phones home.** No telemetry, no analytics, no crash reporting, no
   update pings unless `self_update` is explicitly invoked.
6. **A test per behaviour.** If it changes what a user sees, it has a test that
   would fail if the behaviour broke.

## Adding a note type

1. `types::NoteType`: add the variant, `as_str`, `parse`, `folder`, `stale_key`.
2. `classify::cues`: add the cue table and any structural signal.
3. `config::StaleThresholds`: add the threshold key if the type can go stale.
4. A test in `classify` and one in `stale`.

## Adding a CLI command

`main.rs` uses `argh`: one struct per subcommand with a doc comment on every
field (argh fails the build without help text). Print human output; return
`ExitCode::FAILURE` only for a real failure.
