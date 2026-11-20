# Privacy

Version 1.0 · effective 2026-11-20 · owner: Dr Sohil Momin
What changed: first version.

## The short version

DayDoc is a local program. It has no account, no server, no analytics and no
telemetry. Your notes live in a folder you choose, on your own machine, and
nothing is sent anywhere unless you switch the optional model on and point it at
a provider.

## What is stored, and where

| What | Where | Who can read it |
| --- | --- | --- |
| Your notes | the vault folder you picked, as `.md` files | anyone who can read that folder |
| Attachments | `<vault>/assets/<xx>/<hash>.<ext>` | same |
| Audit log | `<vault>/_meta/audit.jsonl` | same |
| Search index, counters | the state directory (`~/.daydoc` or `$DAYDOC_HOME`) | same |
| Backups | the state directory's `backups/`, unless you pass `--out` | same |
| API and backup keys | the OS keychain, or a `0600` file in the state directory | your user account; the keychain may prompt |
| Broken-note copies | `<vault>/_meta/broken/` | same |

The state directory holds nothing you cannot rebuild from the vault, except the
keys and your backups.

## What leaves the machine

Only one thing, and only if you turn it on:

- **The optional model.** With `llm_mode` set to `byok` or `auto`, `daydoc chat`
  sends your question plus the top-k retrieved notes (capped at 8,000 characters)
  to the provider you configured — OpenAI, Anthropic, Groq, OpenRouter, Gemini, or
  a local server you run. `daydoc chat --preview` prints the exact system prompt
  and note text first, and sends nothing. `llm_send_content = false` blocks the
  send entirely.

There is no update check, no crash reporting, no usage statistics, and no
"anonymous" identifier. `telemetry = true` is refused by the config validator
rather than silently ignored.

## Erasing your data

- **Notes:** delete the vault folder. Nothing outside it is needed to read them.
- **Everything else:** delete the state directory (`~/.daydoc`, or `$DAYDOC_HOME`).
- **Keys:** `daydoc key clear backup` and `daydoc key clear llm`, which remove the
  entry from the keychain as well as the file.
- **Git:** if you enabled `daydoc sync`, deleting a note does not delete its
  history. Rewriting published history is your call, not DayDoc's.

## Personal data and Indian law

DayDoc is software you run, not a service anyone operates, so there is no
controller collecting your data. If you keep other people's personal data in a
vault — meeting notes naming colleagues, for instance — you are the one deciding
what to store and for how long, and the obligations under the Digital Personal
Data Protection Act, 2023 fall on you, not on DayDoc. The features that help are
the ones that already exist: notes are local, `_trash` retention is configurable,
and deletion is a file delete.

This document describes the code as shipped in this repository. If the code
changes, this file changes in the same commit.
