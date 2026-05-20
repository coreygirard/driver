# driver

A minimal task planner for Claude Code, designed to feel native to
`/goal`-style autonomous work without inheriting Conductor's ceremony.

Driver is two pieces:

- **`cli/`** — a small Rust binary (`driver`) that handles all
  mechanical operations on a project's `driver/` directory: parsing
  `plan.md` and `spec.md`, ticking tasks, listing the next runnable
  task, gating the Stop hook, reporting spec gaps.
- **`skills/`** — Claude Code slash commands (`/driver:new`,
  `/driver:spec`, `/driver:status`, `/driver:do`, `/driver:go`) that
  wrap the CLI for interactive use.

Either piece can be used without the other.

## Per-project layout

```
<project>/
  driver/
    plan.md                ← required; the single evolving plan
    spec.md                ← optional; what the project commits to being
    principles.md          ← optional; project-wide rules (file globs)
    <slug>_design.md       ← optional, per task; design notes
    <slug>_questions.md    ← optional, per task; open questions (incl. halt)
    .active                ← runtime; current claim (gitignored)
    .history.jsonl         ← runtime; tick log (gitignored)
```

Driver has three optional layers, each answering one question:

| File             | Question it answers                              | Edited by        |
|------------------|--------------------------------------------------|------------------|
| `spec.md`        | *What* does the project commit to being?         | `/driver:spec`   |
| `plan.md`        | *What* work is in flight (and was done)?         | `/driver:go`, `/driver:new` |
| `principles.md`  | *Which* file changes require explicit consent?   | user, manually   |

**Driver is spec-first.** `/driver:new` runs the spec interrogation
before scaffolding `plan.md`; `/driver:go` synthesizes plan tasks from
unmet spec bullets. The intent is to front-load high-signal
interaction into the spec phase so the implementation phase can run
lights-out. `plan.md` is the only file the CLI strictly requires — a
plan-only project still works — but spec-first is the recommended
flow for anything more than a few tasks.

Plans look like:

```markdown
# Project name

2–3 sentences: what and why.

- [ ] **slug** (~K turns) [depends: other-slug] [spec: a3f2b1]
  Description paragraph.

- [x] **other-slug** (~K turns) [spec: b7c2d8]
  Description paragraph.
```

A task is done when its checkbox is ticked. Slugs must be unique
within `plan.md`. `[spec: <hash>]` is the verifiable link to the spec
bullet a task implements. See `DESIGN.md` for the full data model.

## Spec format (optional)

```markdown
# Project name

1–2 sentences: what this is.

- **<optional-name>** A capability or invariant the project commits to.
  Continuation indented two spaces.
- Bullet without a name. Still gets a content hash.
```

Each bullet has a **content-derived 6-char hash** (FNV-1a, low 24
bits). Editing a bullet's body changes its hash, automatically marking
it as "unmet" until a new task closes it — Unison-style. Names are
optional and not hashed. See `driver spec list` for the current state
and `driver spec amend <old> <new>` if a wording fix shouldn't
invalidate prior work.

## Install

```bash
cd ~/Code/driver  # or wherever you cloned it
cargo install --path cli
ln -sf "$(pwd)/skills" ~/.claude/commands/driver
driver doctor                 # verify the setup; prints the hook snippet if missing
```

The CLI is available as `driver` on PATH. Skills appear as
`/driver:new`, `/driver:status`, etc.

Add `driver/.active` and `driver/.history.jsonl` to your project
`.gitignore`. The first is runtime state for the Stop hook; the second
is a local per-developer log of estimate vs actual turns, used by
`driver stats` to calibrate future estimates.

## The Stop-hook claim mechanism

Driver provides its own analogue of `/goal`: a Stop hook that keeps
the agent working on a claimed task until completion or escalation.

```
driver claim <slug> [--max-turns N]     start a claim
driver gate                              stop-hook callback (do not call directly)
driver release                           end the current claim
```

`--max-turns` defaults to `ceil(estimate × 1.3 / 5) × 5` from the
task's plan estimate, falling back to 50 if the task has no estimate.
Pass it explicitly only when you expect scope growth.

After installing the hook (paste the snippet `driver doctor` prints
when the hook is missing into `~/.claude/settings.json`), each agent
turn ends with `driver gate` running. If a claim is active and the
task isn't ticked or halted, gate exits 2 — Claude Code interprets
this as "keep working." The claim auto-releases when the task is
ticked (`driver tick`), halted by an unanswered `driver ask --halt`
question, budget exhausted, or the **question-rate scope signal**
trips (≥2 new questions accumulating at ≥1/3 per turn — meaning the
spec is under-specifying this task rather than the task being merely
hard). The scope signal prints a distinct message so skills know to
re-spec rather than answer-and-retry.

## CLI commands

```
driver status                              full overview: plan, claim, recent ticks, open questions
driver next [--json]                       next runnable task
driver tasks                               list all tasks with status (done/blocked/runable/waiting)
driver tick <slug>                         mark task done
driver untick <slug>                       mark task open again
driver rename <old-slug> <new-slug>        rename a task (updates deps + per-task files)

driver claim <slug> [--max-turns N]        start a claim (default budget from plan estimate)
driver release                             end the current claim
driver gate                                stop-hook callback
driver doctor                              verify PATH, hook, project layout (prints snippet if hook missing)
driver stats                               estimate-vs-actual from .history.jsonl

driver ask <slug> "<question>" [--rule <name>] [--halt] [--context "..."]
                                           log an open question for batched user review
                                           --halt marks the task halted until answered
driver questions                           list all open questions (answered + unanswered)
driver answer <slug> <Q#> "<answer>" [--promote "<spec-body>"]
                                           record a decision on question Q#
                                           --promote appends <spec-body> as a new spec.md
                                           bullet under `## Architecture`, so future tasks
                                           inherit the decision instead of re-asking

driver spec list                           show spec bullets with hash + met/planned/unmet status
driver spec gap [--json]                   print unmet spec bullets (input to /driver:go synthesis)
driver spec show <hash>                    print one bullet (name, body, tasks referencing it)
driver spec check                          lint each bullet for a concrete verifier signal;
                                           exit non-zero if any bullet has none (test/command/
                                           backticked identifier/predicate/[narrative] tag).
                                           `/driver:go` runs this before synthesis.
driver spec amend <old-hash> <new-hash>    migrate plan.md refs after a cosmetic spec edit
```

`driver tick` always records a completion to `driver/.history.jsonl`.
When there's a matching active claim it captures `{estimate,
actual_turns, budget, estimate_at_claim}` and releases the claim; when
there isn't, `actual_turns` is `null` and tick warns so the loss is
visible. `driver stats` reads that file, ignores no-claim records for
turn-based stats, and flags scope-growth (budget or actual ≥1.5× the
plan estimate captured at claim time). Both `.active` and
`.history.jsonl` are gitignored — they're per-developer runtime state.

Run from anywhere inside a project — `driver` walks up looking for
`driver/plan.md` (or `driver/principles.md`).

## Skills

| Slash command | What it does |
|---|---|
| `/driver:new` | Bootstraps a project: runs the spec interrogation first, then scaffolds empty `plan.md`; commits |
| `/driver:spec` | Interrogates the user across {target, shape, architecture, out-of-scope}; writes/extends `spec.md`; commits |
| `/driver:status` | Wrapper around `driver status` |
| `/driver:do` | Runs the next runnable task end-to-end (Stop-hook safety net) |
| `/driver:go [guidance] [--dry-run]` | Synthesizes tasks from `spec.md` gaps, then runs all runnable tasks. `--dry-run` prints the proposed task block and stops — no plan.md edit, no claim, no execution. |

The intended workflow is: `/driver:new` once per project (the spec is
the high-signal conversation), then `/driver:go` until the spec is
fully met, then `/driver:spec` again to extend the contract when you
want to grow the project.

## Principles + the autonomy floor

Driver's autonomy rubric has two layers:

- **Mechanical floor**: optional `driver/principles.md` lists rules with
  file globs. If a task's diff (since claim start) touches a triggered
  file, `driver tick` refuses unless a `driver ask --rule <name>`
  question is logged. Leak-proof — the agent can't bypass it.
- **Self-classification ceiling**: agent uses `driver ask` *without*
  `--rule` for things that are architecturally consequential but not
  caught by a glob (lossy approximations, representation choices,
  symmetry breaks). The agent's judgment; over-asking is encouraged.

Questions land in `<slug>_questions.md` with a `**answer:** _pending_`
marker. When `driver tick` refuses on unanswered questions, the
`/driver:do` and `/driver:go` skills walk the user through each one
*conversationally* — present the question and recommendation, pause
for discussion, then record the user's decision via `driver answer`.
`/driver:go` batches all open questions across the whole run before
opening the conversation, so the user reviews everything in one
sitting.

See `DESIGN.md` for the full rationale.

## License

MIT.
