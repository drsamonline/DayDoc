# Security policy

Version 1.0 · effective 2026-11-20 · owner: Dr Sohil Momin
What changed: first version.

## Reporting a vulnerability

Email **sohilmomin2000@gmail.com** with the details. Please do not open a public
issue for anything that could put a user's notes or keys at risk.

Include what you did, what happened, the version (`daydoc status` prints it) and
your platform. A proof of concept helps; a description is enough to start.

What to expect: an acknowledgement within 7 days, an assessment within 30, and a
fix or a reasoned refusal. I will credit you in the release notes unless you ask
me not to. There is no bug bounty.

## What DayDoc protects

- **Your notes.** The vault is plain Markdown on your disk. There is no account,
  no server and no telemetry. Nothing leaves the machine unless you configure the
  optional model.
- **Write integrity.** Every note write is staged in a write-ahead log, checksummed
  and committed by rename. A crash replays on start; a corrupt log entry is set
  aside and the vault is left untouched.
- **Path safety.** Vault paths are relative only. `..`, absolute paths and
  symlinks are refused, and deleting a note verifies the canonical path is inside
  the vault.
- **Keys.** API and backup keys are stored in the OS keychain where one is
  available, otherwise in a `0600` file in the state directory. Keys are never
  written to the vault, to `daydoc.toml`, to the audit log, or to a preview.
- **Backups.** With `backup_encrypt = true`, archives are AES-256-GCM with a fresh
  nonce per file. A wrong key or a tampered archive fails closed, and DayDoc
  refuses to write an unencrypted archive while encryption is switched on.

## What DayDoc does not protect

Be clear-eyed about this before you keep anything sensitive in a vault.

- **It is not encrypted at rest by default.** Vault files are ordinary Markdown.
  Anyone who can read your home directory can read your notes. Use full-disk
  encryption, and set `backup_encrypt = true` for archives.
- **The `0600` key file protects against other accounts, not against you.** On a
  machine without a usable keychain, a process running as your user can read the
  key. Use the keychain, or supply keys through the environment for a single run.
- **The optional model sees what you send it.** With `llm_mode` on and
  `llm_send_content = true`, the retrieved notes go to the provider you configured.
  `daydoc chat --preview` shows the exact text first. `llm_send_content = false`
  refuses to send anything.
- **Git history is forever.** If you enable `daydoc sync`, anything you ever
  committed is in that history, including a note you later deleted. Do not commit
  secrets.
- **Attachments are not content-filtered.** `daydoc attach` stores a file by hash;
  it does not scan it.

## Supported versions

The latest release on the `main` branch. This is a single-maintainer project with
no long-term support branches; fixes land on `main` and ship in the next tag.

## Out of scope

Vulnerabilities that require an attacker to already control your user account,
your filesystem, or your operating system. Reports about the absence of features
listed in "What DayDoc does not protect" are not vulnerabilities; they are the
documented design.
