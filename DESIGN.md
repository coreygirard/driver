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

## Autonomy rubric: mechanical floor + self-classification ceiling

The earlier prose-only rubric ("decide reversible things, escalate
irreversible ones") leaked. The agent self-classified, and the agent
had an incentive to keep moving. Driver now layers two enforcement
mechanisms:

### Mechanical floor (CLI-enforced, leak-proof)

`driver/principles.md` lists named rules, one file glob per rule:

```markdown
- name: core-ir-schema
  glob: src/core_ir.rs
  description: any change to CoreIr types
```

At claim time, `driver claim` records the current git HEAD as
`start_commit`. At tick time, `driver tick` runs
`git diff --name-only <start_commit>..HEAD` and matches each touched
file against every rule's glob. For each tripped rule, tick verifies
that `<slug>_questions.md` contains a question with `**rule:** <name>`.
If not, tick refuses with a diagnostic.

The agent cannot bypass this. The rules are project-level config — they
generalize as the user adds globs, and don't bake project specifics
into the skill.

### Self-classification ceiling (agent judgment, additive)

`driver ask <track> <slug> "<question>" --context "..."` *without*
`--rule` is the channel for things the agent thinks are
architecturally consequential but no glob caught: lossy
approximations, representation choices, cross-language symmetry
breaks, naming a public concept. The agent's judgment; over-asking is
encouraged because asking is cheap (one CLI call, doesn't halt).

### The "ask, don't block" loop

`driver ask` is non-blocking. The task remains open (work is committed,
tick refuses, agent moves on). Other tasks can still be claimed and
worked. `/driver:go` keeps looping until no task can advance, then
batches all open questions into one end-of-run report. The user
reviews and answers everything in one pass, then re-runs `/driver:go`
and staged tasks resume.

`driver block` still exists for the "fully stuck, can't even commit
partial work" case. Different channel, harder block.

### What still goes in `decisions.md`

One-line entries for reversible local choices: variable names,
internal helpers, fixture content within an existing pattern, library
trade-offs. The user reviews after the run and can `git revert`
anything they don't like.

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

- Multi-`*` globs in principles.md (e.g. `src/**/*.rs`). v1 supports
  one `*` per pattern, which covers exact-file rules and one-level
  wildcards like `testdata/eval/*.json`.
- Some skills still re-parse plan.md inline rather than shelling out to
  the CLI — `/driver:status` already delegates; others should follow.
- Multi-developer collaboration. `.active`, `.history.jsonl`, and the
  questions files are local. If two people share a `driver/` repo,
  conflicts on `<slug>_questions.md` are possible. Not worth solving
  until it's a real problem.

These are tracked work, not gaps in the design.
