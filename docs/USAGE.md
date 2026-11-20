# Usage

Ten things DayDoc is for, each with the commands that do it. The CLI and the
window use the same library, so anything here works either way.

## 1. Record a decision

```bash
daydoc capture "we decided to adopt Okta for SSO across all products"
# decisions/DR-0001.md — status: proposed
daydoc accept DR-0001
```

A capture that the classifier is not sure about lands in `inbox/` with a type
badge. `daydoc capture --preview "…"` shows the guess without writing.

## 2. Replace a decision without losing the old one

```bash
daydoc supersede DR-0001 "Use Entra ID for SSO" --decision "Entra is cheaper at our volume"
# DR-0002 supersedes DR-0001; both stay readable, both carry the link
daydoc revert DR-0002 --reason "wrong call, Okta stays"
# both accepted again, links cleared, the change audited
```

## 3. Turn a meeting into decisions

```bash
daydoc meeting meetings/kickoff.md                 # candidate cards, nothing written
daydoc meeting meetings/kickoff.md --approve 1,2 --actions
```

Each candidate is one line: `Adopt Okta for SSO — approve as DR-0012?`. `--actions`
also writes the extracted action items into the meeting note as checkboxes.

## 4. Ask what was decided

```bash
daydoc ask "what did we decide about okta"
```

You get the card, not prose: reference, status, owner, what it replaced, what
cites it. `daydoc search okta --type decision` is the same query with filters.

## 5. Hand over a period

```bash
daydoc handover --from 2026-11-01 --to 2026-11-20
daydoc wrap        # end-of-day, written into today's daily note
daydoc digest      # the week, written into daily/digests/
```

All three are Markdown, all three accept `--json` when a script needs the facts.

## 6. Keep the vault honest

```bash
daydoc stale     # age by type, plus notes that contradict the ledger
daydoc verify reference/old-doc.md
daydoc lint      # ids, types, citations, links, attachments
daydoc lint --fix  # assign a ULID to notes that have none, and nothing else
```

A note that cites a superseded decision, names a renamed project or is owned by
someone who has left is flagged, not edited.

## 7. Bring an existing folder in

```bash
daydoc import ~/obsidian-vault --dry-run   # what would happen
daydoc import ~/obsidian-vault             # paths kept, ids assigned, nothing overwritten
```

Symlinks are not followed. A file that already exists is skipped, not clobbered.

## 8. Protect the vault

```bash
daydoc key set backup --generate       # prints the key once; store it elsewhere too
daydoc config set backup_encrypt true
daydoc backup                          # backups/daydoc-2026-11-20.tar.gz.enc
daydoc restore backups/daydoc-2026-11-20.tar.gz.enc --into /tmp/restored
```

Retention is 7 daily, 4 weekly, 6 monthly. `restore` refuses a folder that
already has files unless you pass `--force`.

## 9. Capture from a phone

```bash
daydoc config set mobile_enabled true
daydoc key set mobile --generate
daydoc mobile --addr 0.0.0.0:7373
```

Open the address on your phone: one box, one token, an offline queue. Reading
from the phone needs a second token and `mobile_capture_only = false`.

## 10. Work in an editor

```bash
daydoc watch      # reindex on external edits, commit every 300 s if git is set up
daydoc ui         # or the window: Ctrl+Shift+Space to capture, Ctrl+Shift+K to search
```

The vault is plain files. VS Code, Obsidian and `vim` all work; the watcher picks
the changes up.

To keep it running all day, `contrib/` has a systemd user unit and a macOS
launchd plist, and `contrib/README.md` says what the watcher does and does not do.
