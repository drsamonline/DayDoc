# Running DayDoc all day

Two files, for the two init systems people actually use on a work machine. Neither
is installed by anything; copy the one you need and edit the paths.

## Linux, systemd

```bash
cp contrib/daydoc.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now daydoc.service
journalctl --user -u daydoc -f
```

## Windows

There is no service unit here. Put a shortcut to `daydoc.exe -v C:\notes watch`
in `shell:startup`, or add a scheduled task at logon with "Run whether user is
logged on or not" off.

## What the watcher does

It indexes notes other programs write, and commits the vault every
`auto_commit_debounce_secs` (300 by default) if the vault is a git repository. It
ignores its own bookkeeping, so a commit cannot trigger the next commit.

Nothing here runs `daydoc ui`; that is a window you open yourself.
