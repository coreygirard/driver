# Driver design notes

Captures the design conclusions Driver was built on, so future work
doesn't re-derive them from scratch.

## Purpose

Driver is a minimal track planner for `/goal`-driven work in Claude
Code. It exists because the existing alternatives (Conductor,
Conductor2) carry more structure than a single-human + single-agent
workflow actually needs.

Driver's premise: most of the value of Conductor came from one file
(`plan.md`). Everything else — `metadata.json`, per-track `index.md`,
`notes.md`, a universal file-resolution protocol, top-level
`product.md` / `tech-stack.md` / `workflow.md` — turned out to be
ceremony that didn't pay back.

## What Driver is

Two pieces:

- **`cli/`** — a small Rust binary that parses `driver/` project
  files and performs mechanical operations (list runnable tasks, tick
  task as done, close a track).
- **`skills/`** — Claude Code slash commands that wrap the CLI and
  add LLM-judgment features (drafting `/goal` prompts, scaffolding
  new tracks, deciding when a task needs an upfront design doc).

Either piece is usable without the other.

## Per-project layout

```
<project>/
  driver/
    tracks.md                      ← registry
    tracks/
      <YYYYMMDD>-<slug>/
        plan.md                    ← required
        spec.md                    ← optional ("why")
        <task-slug>_design.md      ← optional, per task
        <task-slug>_blocked.md     ← optional, per task; pauses that task
        decisions.md               ← optional, appended by /driver:go
    principles.md                  ← optional, project-wide rules
```

## Plan format: task-flat DAG

Plans are flat lists of tasks. Tasks are the unit of `/goal` dispatch.
There are no "phases" or sub-bullets; if a task is too big, split it
into multiple tasks.

```markdown
# Track name

Intro paragraph: what, why.

- [ ] **slug** (~K turns) [depends: other-slug, another-slug]
  Description paragraph. Enough for an agent firing a /goal to know
  what's expected; details can live in <slug>_design.md.

- [x] **other-slug** (~K turns)
  Description paragraph.
```

- The slug is the identifier. Stable across reorderings.
- Estimate is in parens.
- `[depends: …]` is optional. Default = no dependencies = always runnable.
- Description is one paragraph. If a task needs more spec, write a
  `<slug>_design.md` and reference it from the description.

## Why slugs and not UUIDs

Slugs are readable. UUIDs would solve a collision problem that
single-author markdown plans don't actually have, and pay for it with
unreadable `[depends: 5f3e2a1b-…]` annotations. Rename safety is
handled by `driver rename <track> <old-slug> <new-slug>`, which
updates plan.md and renames associated files.

If Driver ever gets multi-author concurrent edits or cross-project
references, UUIDs become the right call. Driver isn't there.

## Why a DAG and not a list

A blocked task shouldn't halt unrelated work. With a flat list,
"blocked" is global. With a DAG, "blocked" is a subtree. The user
authoring overhead is one optional `[depends: …]` annotation per
task that has actual dependencies; most tasks have none.

The DAG also gives `/driver:go` natural semantics: "iterate runnable
tasks until none remain."

## Two states, not three

A task is open or done. No "in progress" middle state — by definition
the next runnable open task is the one currently being worked on.

A track is open or done. A track is closeable when every task in its
plan.md is done.

## Autonomy rubric

When an agent runs `/driver:go`, it decides reversible things and
escalates irreversible ones:

- **Reversible (decide and log to decisions.md)**: naming, file
  structure, internal helpers, test fixture choices, library/dep
  choices that don't lock long-term.
- **Hard-to-reverse (write `<slug>_blocked.md` and stop this task)**:
  public API changes, schema changes, lexicon shape changes,
  deletions, architectural choices that affect multiple future tasks.

When in doubt: "can a future task undo this with a small diff?"
If yes, decide. If no, escalate.

## What Driver deliberately doesn't have

- `metadata.json` per track
- per-track `index.md`
- per-track `notes.md`
- File resolution protocol or default-path tables
- Project-level `product.md`, `tech-stack.md`, `workflow.md`
- Multiple track types (`feature`, `chore`, `bug`)
- `[~]` "in-progress" state in registries
- Sub-task bullets within tasks (use TaskCreate during a `/goal`)
- Subagent dispatch infrastructure (single conversation is the
  manager, `/goal` is the execution primitive)

## What Driver hasn't yet added

- `principles.md` template and `/driver:go` skill — pending.
- `driver rename` CLI command — pending.
- `<slug>_blocked.md` semantics in CLI (currently per-track) — pending.
- Skills shelling out to the CLI for parsing — currently each layer
  parses independently.

These are tracked work, not gaps in the design.
