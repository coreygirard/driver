# Driver design notes

Captures the design conclusions Driver was built on, so future work
doesn't re-derive them from scratch.

## Purpose

Driver is a minimal task planner for `/goal`-driven work in Claude
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
  task as done, gate the Stop hook).
- **`skills/`** — Claude Code slash commands that wrap the CLI and
  add LLM-judgment features (scaffolding `plan.md`, deciding when a
  task needs an upfront design doc, conversational question
  walkthroughs).

Either piece is usable without the other.

## Per-project layout

```
<project>/
  driver/
    plan.md                ← required; the single evolving plan
    <task-slug>_design.md  ← optional, per task
    <task-slug>_questions.md  ← optional, per task; open questions (incl. halt)
    principles.md          ← optional; project-wide rules
```

## One plan, no tracks

Driver is a single evolving `plan.md`. There are no per-chunk
directories, no registry file, no "open the next track" ritual.
Completed tasks stay in `plan.md` as ticked history; new work appends
bullets. Slugs are unique within the file.

An earlier design had a per-track filesystem layout
(`driver/tracks/<YYYYMMDD>-<slug>/plan.md` + sibling files). Real use
exposed several costs: every per-task CLI command carried a `<track>`
argument; `/driver:new` performed a closure ritual that paralleled
plan.md but lived elsewhere; `[depends: …]` couldn't cross "chunk"
boundaries even when later work genuinely required earlier
infrastructure. The grouping affordance tracks gave (visually
clustering related work) is recovered by optional `## Section`
markdown headers in `plan.md`, with no parser awareness needed.

## Plan format: task-flat DAG

Plans are flat lists of tasks. Tasks are the unit of `/goal` dispatch.
There are no "phases" or sub-bullets; if a task is too big, split it
into multiple tasks.

```markdown
# Project name

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
handled by `driver rename <old-slug> <new-slug>`, which updates
plan.md and renames associated per-task files.

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

`driver ask <slug> "<question>" --context "..."` *without* `--rule` is
the channel for things the agent thinks are architecturally
consequential but no glob caught: lossy approximations, representation
choices, cross-language symmetry breaks, naming a public concept. The
agent's judgment; over-asking is encouraged because asking is cheap
(one CLI call, doesn't halt).

### The "ask, don't block" loop — and one channel for halt

`driver ask` is the only escalation channel. By default it's
non-blocking: the task remains open (work is committed, tick refuses,
agent moves on), other tasks can still be claimed and worked.
`/driver:go` keeps looping until no task can advance, then switches
into a *conversational walkthrough* with the user over the
accumulated open questions — one at a time, with context and the
agent's recommendation, pausing for discussion. When the user decides,
the skill records the answer via `driver answer <slug> <Q#>
"<decision>"`. Once all answered, `/driver:go` resumes the loop with
the now-runnable staged tasks.

For the "fully stuck, no further progress on this task" case, pass
`--halt`: the question is logged with `**halt:** true`, the active
claim is released, and the task is treated as blocked (`is_runnable`
returns false; gate releases; dependents are transitively
unreachable). Answering the halt-question clears the halt
automatically.

There used to be a separate `driver block` / `<slug>_blocked.md`
channel. Real dogfooding never fired it — `ask` covered every case —
and "task with unanswered halt-question" gives the same
downstream-unreachable semantics with one fewer file convention and
one fewer CLI command.

### Reversible local calls stay out of `ask`

Variable names, internal helpers, fixture content within an existing
pattern, library trade-offs — just do them, and let the commit
message carry the rationale if it's non-obvious. The user reviews via
`git log` / `git diff` after the run and can `git revert` anything
they disagree with.

## What Driver deliberately doesn't have

- `tracks.md` registry (and `tracks/<id>/` per-chunk folders) — one
  evolving `plan.md` instead
- `metadata.json` per task
- per-track `index.md`, `notes.md`, `decisions.md`
- File resolution protocol or default-path tables
- Project-level `product.md`, `tech-stack.md`, `workflow.md`
- Multiple task types (`feature`, `chore`, `bug`)
- `[~]` "in-progress" state
- Sub-task bullets within tasks (use TaskCreate during a `/goal`)
- Subagent dispatch infrastructure (single conversation is the
  manager, `/goal` is the execution primitive)
- A `close` verb — done-ness is just "all tasks ticked"

## The claim / gate / release mechanism

Driver provides its own analogue of `/goal`'s Stop hook, built from
Driver primitives rather than depending on `/goal`.

A *claim* records `{slug, max_turns, turn, started_at,
estimate_at_claim, start_commit}` in `driver/.active` when
`driver claim <slug>` runs. The Stop hook (configured globally via
`~/.claude/settings.json`) runs `driver gate` after every agent turn
end. The gate:

1. Reads `driver/.active`. If missing → exit 0, normal stop.
2. Increments turn count.
3. If the task is now ticked in `plan.md` → release + exit 0.
4. If `<slug>_questions.md` has any unanswered halt-question → release + exit 0.
5. If turn > max_turns → release + exit 0 (with a warning).
6. Otherwise → exit 2 with a stderr message, blocking the stop.

This buys back `/goal`'s most useful property — "keep working until
the task is done" — without depending on `/goal`. Skills like
`/driver:do` can now run a task end-to-end in one command, with no
paste step. The gate is deterministic (file-state check, no LLM
evaluator), simpler than `/goal`'s Haiku-judging condition.

Don't combine with `/goal` in the same session: both set Stop hooks
and the interaction is undefined. Pick one per session.

## Calibration: history + stats

`driver tick` *always* appends to `driver/.history.jsonl` when ticking
to done. The record carries `{ts, slug, estimate, actual_turns,
budget, estimate_at_claim, status}`:

- With a matching active claim, all fields are populated; tick also
  deletes `.active`.
- Without one, `actual_turns`, `budget`, and `estimate_at_claim` are
  `null`, and tick prints a one-line warning. The completion is still
  recorded so `driver stats` can count tasks shipped, just not their
  duration.

The earlier "tick only records under a claim" rule lost calibration
data silently — a real run shipped four tasks with an empty history
file. Always-record + visible warning makes the failure mode legible.

`driver stats` reports mean/median turns (over records that have
`actual_turns`), mean actual/est ratio, and the three biggest over-
and under-estimates. It also surfaces **scope-growth**: tasks where
the claim budget or actual turns exceeded `estimate_at_claim` by
≥1.5×. That catches mid-flight estimate bumps even when the user
edited plan.md during the run.

This is the only thing that ever fixes estimate calibration. Without
it, the `~K turns` annotation in plan.md is unfalsifiable.

Both `.active` and `.history.jsonl` are per-developer runtime state and
should be gitignored. Sharing `.history.jsonl` across collaborators
would conflate execution speeds and isn't the goal — each developer
calibrates their own pace.

## One overview: `driver status`

`driver status` is the single command for "what's going on?" It prints,
in order: the plan summary (with next-runnable inline), the active
claim (if any), the last five ticks from history, and all unanswered
open questions. Sufficient both as a daily check and as session
re-entry after Claude Code context compaction.

An earlier design split this across three commands: `status` for a
per-track grid, `claim-status` for the active claim, and `resume` for
the session re-entry composite. Real use exposed the overlap — the
three views shared most of their data — and forced the user to
remember which command answered which question. Collapsing into one
removes that decision; the data is the same, the section headers
(`== plan ==`, `== active claim ==`, etc.) keep the output scannable.

## Doctor

`driver doctor` is the one-shot setup check used by `/driver:do` and
`/driver:go` as a preflight. It verifies:

- `driver` is on PATH (catches the common `~/.cargo/bin` issue).
- `~/.claude/settings.json` exists and contains a Stop hook entry
  invoking `driver gate`.
- The current working directory is inside a `driver/` project
  (i.e. has `plan.md` or `principles.md`).
- Reports the active claim if any.

Exits 0 if global setup is OK (project not required); exits 1 if any
of the global checks fails.

## What Driver hasn't yet added

- Multi-`*` globs in principles.md (e.g. `src/**/*.rs`). v1 supports
  one `*` per pattern, which covers exact-file rules and one-level
  wildcards like `testdata/eval/*.json`.
- CLI tests. `cli/src/main.rs` is one file with no `#[cfg(test)]`
  module yet. The parsers (`parse_tasks`, `parse_depends`,
  glob-matching, `rewrite_depends`, transitive-blocker walk) and the
  `gate` state machine are load-bearing and unprotected.
- Multi-developer collaboration. `.active`, `.history.jsonl`, and the
  questions files are local. If two people share a `driver/` repo,
  conflicts on `<slug>_questions.md` are possible. Not worth solving
  until it's a real problem.

These are tracked work, not gaps in the design.
