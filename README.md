# driver

A minimal task planner for Claude Code, designed to feel native to
`/goal`-style autonomous work without inheriting Conductor's ceremony.

Driver is two pieces:

- **`cli/`** — a small Rust binary (`driver`) that handles all
  mechanical operations on a project's `driver/` directory: parsing
  `plan.md`, ticking tasks, listing the next runnable task, gating the
  Stop hook.
- **`skills/`** — Claude Code slash commands (`/driver:new`,
  `/driver:status`, `/driver:do`, `/driver:go`) that wrap the CLI for
  interactive use.

Either piece can be used without the other.

## Per-project layout

```
<project>/
  driver/
    plan.md                ← required; the single evolving plan
    <slug>_design.md       ← optional, per task; design notes
    <slug>_questions.md    ← optional, per task; open questions (incl. halt)
    principles.md          ← optional; project-wide rules
    .active                ← runtime; current claim (gitignored)
    .history.jsonl         ← runtime; tick log (gitignored)
```

Driver is **one evolving plan** — there are no per-track folders, no
registry file to maintain. New work appends bullets to `plan.md`;
completed work stays as ticked history.

Plans look like:

```markdown
# Project name

2–3 sentences: what and why.

- [ ] **slug** (~K turns) [depends: other-slug, another-slug]
  Description paragraph.

- [x] **other-slug** (~K turns)
  Description paragraph.
```

A task is done when its checkbox is ticked. Slugs must be unique
within `plan.md`. See `DESIGN.md` for the full data model.

## Install

```bash
cd ~/Code/driver  # or wherever you cloned it
cargo install --path cli
ln -sf "$(pwd)/skills" ~/.claude/commands/driver
driver init-hook              # then paste the snippet into ~/.claude/settings.json
driver doctor                 # verify the setup
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
driver claim <slug> --max-turns <N>     start a claim
driver gate                              stop-hook callback (do not call directly)
driver release                           end the current claim
driver init-hook                         print the settings.json snippet
```

After installing the hook (via `driver init-hook` + paste, or by
adding the snippet to `~/.claude/settings.json` directly), each agent
turn ends with `driver gate` running. If a claim is active and the
task isn't ticked or halted, gate exits 2 — Claude Code interprets
this as "keep working." The claim auto-releases when the task is
ticked (`driver tick`), halted by an unanswered `driver ask --halt`
question, or budget exhausted.

## CLI commands

```
driver status                              full overview: plan, claim, recent ticks, open questions
driver next [--json]                       next runnable task
driver tasks                               list all tasks with status (done/blocked/runable/waiting)
driver tick <slug>                         mark task done
driver untick <slug>                       mark task open again
driver rename <old-slug> <new-slug>        rename a task (updates deps + per-task files)

driver claim <slug> --max-turns N          start a claim (Stop hook gate)
driver release                             end the current claim
driver gate                                stop-hook callback
driver init-hook                           print settings.json snippet
driver doctor                              verify PATH, hook, project layout
driver stats                               estimate-vs-actual from .history.jsonl

driver ask <slug> "<question>" [--rule <name>] [--halt] [--context "..."]
                                           log an open question for batched user review
                                           --halt marks the task halted until answered
driver questions                           list all open questions (answered + unanswered)
driver answer <slug> <Q#> "<answer>"       record a decision on question Q#
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
| `/driver:new` | Asks for title + task outline; scaffolds `plan.md`; commits |
| `/driver:status` | Wrapper around `driver status` |
| `/driver:do` | Runs the next runnable task end-to-end (Stop-hook safety net) |
| `/driver:go` | Runs *all* runnable tasks until done or halted |

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
