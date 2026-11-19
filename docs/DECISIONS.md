# DayDoc's own ledger

The project eats its own format. These are the decisions taken while building
DayDoc, in the shape DayDoc stores them.

---

**DR-0001 — Store the vault as plain Markdown plus YAML**
Accepted · owner: maintainers

*Context.* A notes tool that owns its data format needs an export story, a
migration story and a corruption story. All three are expensive and all three
are avoidable.

*Alternatives.* A single SQLite database (fast queries, opaque data); a JSON
document store (diffable, but every editor mangles it).

*Decision.* One `.md` file per note with YAML frontmatter. The index lives
outside the vault and is rebuildable. Uninstalling DayDoc loses nothing.

*Consequences.* Search needs its own index, and frontmatter has to be written
deterministically so diffs stay small. Both are accepted costs.

---

**DR-0002 — Two ID spaces: ULID for linking, `DR-NNNN` for humans**
Accepted

*Context.* A single ID cannot be both stable across renames and readable in a
meeting.

*Decision.* `id` is a ULID (opaque, stable). `decision_id` is `DR-NNNN` (the
human reference and the supersession pointer). Paths are mutable index fields.

*Consequences.* Every decision carries two identifiers and the code must never
conflate them. Numbers are never reused, even after a note is deleted.

---

**DR-0003 — Supersession must be revertible**
Accepted

*Context.* A wrong supersession silently buries a decision that was correct. The
first design made supersession a one-way door.

*Decision.* A revert clears the link on both sides, restores both decisions to
`accepted`, and writes an audit entry. The superseding decision stays in the
ledger as its own record.

*Consequences.* Revert is the only backwards edge in the lifecycle, and it is
narrow: one link, two notes, one audit line.

---

**DR-0004 — Tier 0 before any model**
Accepted

*Context.* Bundled embeddings and an optional LLM are both in scope. Shipping
classification, extraction and staleness only through a model would make the
product unusable on a machine without the model, and untestable offline.

*Decision.* Every core feature works with no model, no network and no download:
a weighted cue table for intent, ordered n-grams for SOP matching, explicit
rules for staleness. Models and LLMs are additive and off by default.

*Consequences.* Extraction is less clever than an LLM would be, and the cue
table is English-only at v1. Accepted until the bundled models land.

---

**DR-0005 — Staged writes, not direct writes**
Accepted

*Context.* Direct `fs::write` into a vault loses data on a crash mid-write, and
Windows file locks make a naive rename fail when an editor holds the file.

*Decision.* Every write stages a checksummed entry in a write-ahead log, is
fsync'd, then commits by rename; a crash replays on start. Windows sharing
violations (os error 32/33) are retried with 1→16 s backoff.

*Consequences.* One extra fsync per write, and a state directory outside the
vault. Both are cheap next to losing a decision.
