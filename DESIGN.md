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

## The claim / gate / release mechanism

Driver provides its own analogue of `/goal`'s Stop hook, built from
Driver primitives rather than depending on `/goal`.

A *claim* records `{track, slug, max_turns, turn, started_at}` in
`driver/.active` when `driver claim <track> <slug>` runs. The Stop
hook (configured globally via `~/.claude/settings.json`) runs
`driver gate` after every agent turn end. The gate:

1. Reads `driver/.active`. If missing → exit 0, normal stop.
2. Increments turn count.
3. If the task is now ticked in `plan.md` → release + exit 0.
4. If `<slug>_blocked.md` exists → release + exit 0.
5. If turn > max_turns → release + exit 0 (with a warning).
6. Otherwise → exit 2 with a stderr message, blocking the stop.

This buys back `/goal`'s most useful property — "keep working until
the task is done" — without depending on `/goal`. Skills like
`/driver:do` can now run a task end-to-end in one command, with no
paste step. The gate is deterministic (file-state check, no LLM
evaluator), simpler than `/goal`'s Haiku-judging condition, and the
escalation channel (`<slug>_blocked.md`) is the same as `/driver:next`
already uses.

Don't combine with `/goal` in the same session: both set Stop hooks
and the interaction is undefined. Pick one per session.

## Calibration: history + stats

`driver tick` appends `{ts, track, slug, estimate, actual_turns,
status}` to `driver/.history.jsonl` when the ticked task matches the
active claim, then deletes `.active`. `driver stats [<track>]` reads
that file and reports mean/median turns, mean actual/est ratio, and
the three biggest over- and under-estimates by slug.

This is the only thing that ever fixes estimate calibration. Without
it, the `~K turns` annotation in plan.md is unfalsifiable.

Both `.active` and `.history.jsonl` are per-developer runtime state and
should be gitignored. Sharing `.history.jsonl` across collaborators
would conflate execution speeds and isn't the goal — each developer
calibrates their own pace.

## Doctor

`driver doctor` is the one-shot setup check used by `/driver:do` and
`/driver:go` as a preflight. It verifies:

- `driver` is on PATH (catches the common `~/.cargo/bin` issue).
- `~/.claude/settings.json` exists and contains a Stop hook entry
  invoking `driver gate`.
- The current working directory is inside a project with
  `driver/tracks.md`.
- Reports the active claim if any.

Exits 0 if global setup is OK (project not required); exits 1 if any
of the global checks fails.

## What Driver hasn't yet added

- `driver/principles.md` template — pending, but not blocking.
- Some skills still re-parse plan.md inline rather than shelling out to
  the CLI — `/driver:status` already delegates; others should follow.
- The autonomy rubric in `/driver:do` is descriptive prose, not a
  contract the CLI can enforce. A plan.md annotation
  (`[design-pass: yes]`, `[ask-first: <topic>]`) parsed by the CLI
  would make the rubric machine-checkable. See the open conversation
  about Driver improvements.

These are tracked work, not gaps in the design.
