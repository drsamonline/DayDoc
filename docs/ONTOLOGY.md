# The ontology

DayDoc is a decision record with notes attached, not a note pile with a
decisions folder. Everything follows from four object kinds and the links
between them.

## Objects

### Decision

An atomic object with an identity, a lifecycle and a history.

```yaml
---
id: 01J8ZZ3WQ4T9R6M2XK7B5N0PVD        # ULID: internal, stable across renames
type: decision
decision_id: DR-0007                    # human reference, supersession pointer
title: Use Okta for SSO
status: accepted                        # proposed | accepted | superseded | rejected
owner: alice
decided_at: 2026-03-04
supersedes: DR-0003
superseded_by:                        # set when a later decision replaces this one
superseded_at:
source: meetings/2026-03-04-sso.md
last_verified: 2026-03-04
---
## Context
## Alternatives
## Decision
## Consequences
```

Rules:

- `id` and `decision_id` never conflate. Path is a mutable index field; rename
  the file and nothing breaks.
- `proposed` → `accepted` or `rejected`. `accepted` → `superseded` only, and
  only through a new decision. `superseded` → `accepted` only through a revert.
- An accepted decision never goes stale. It is either current or superseded.
  A proposed decision does go stale (90 days by default): an undecided question
  is a liability, not a record.

### Evidence

Any other note: a meeting, a project update, a reference, an idea, a daily note.
Its relationship to a decision is a citation (`DR-0007` in the body) or a link
(`links:` in frontmatter). Evidence is what makes a decision checkable six
months later.

Evidence can contradict the ledger. A note that still cites `DR-0003` after
`DR-0007` superseded it is flagged — not edited. The user decides whether the
note is wrong, or the supersession was.

### Procedure (SOP)

A sequence of steps that came out of repeated decisions. When three or more
notes describe the same **ordered** sequence, DayDoc proposes a draft. The
proposal carries provenance and a `last_verified` date, because a procedure
nobody has run in six months is a hazard.

### Task

A checkbox derived from a decision or a meeting. Tasks are the only object that
disappears when done; decisions stay forever.

## Links

| From | To | How |
| --- | --- | --- |
| Evidence | Decision | `DR-NNNN` citation in the body, or `links:` |
| Decision | Decision | `supersedes` / `superseded_by` |
| SOP | Evidence | `links:` back to the notes it was extracted from |
| Task | Decision | citation in the task line or its note |

## The lifecycle in practice

```
proposed ──accept──> accepted ──supersede──> superseded
   │                    ^                        │
   └──reject──> rejected│                        │
                        └────────revert──────────┘
```

Revert is the only edge that moves backwards, and it is deliberately narrow: it
clears one supersession link on both notes, restores both to `accepted`, and
writes an audit entry. Nothing is deleted, and the reverted decision remains in
the ledger as its own record.

## Why this is the product

A notes app answers "what did I write". DayDoc answers:

- What did we decide, and is it still true?
- What replaced it, and when?
- Which notes still argue for the old answer?
- What is open, and who owns it?

Those four questions are why the ledger is the schema rather than a feature.
