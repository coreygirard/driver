# driver

A minimal track planner for Claude Code, designed to feel native to
`/goal`-style autonomous work without inheriting Conductor's ceremony.

Driver is two pieces:

- **`cli/`** — a small Rust binary (`driver`) that handles all
  mechanical operations on a project's `driver/` directory: parsing
  plans, ticking tasks, listing the next runnable task, gating Stop
  hooks, closing tracks.
- **`skills/`** — Claude Code slash commands (`/driver:new`,
  `/driver:status`, `/driver:next`, `/driver:close`, `/driver:do`,
  `/driver:go`) that wrap the CLI for interactive use.

Either piece can be used without the other.

## Per-project layout

```
<project>/
  driver/
    tracks.md                      ← registry; one bullet per track
    tracks/
      <YYYYMMDD>-<slug>/
        plan.md                    ← required
        spec.md                    ← optional; the "why"
        <slug>_design.md           ← optional; per-task design notes
        <slug>_blocked.md          ← optional; pauses that task
        decisions.md               ← optional; appended by /driver:go
    principles.md                  ← optional; project-wide rules
    .active                        ← runtime; the current claim (gitignored)
```

Plans look like:

```markdown
# Track name

2–3 sentences: what and why.

- [ ] **slug** (~K turns) [depends: other-slug, another-slug]
  Description paragraph.

- [x] **other-slug** (~K turns)
  Description paragraph.
```

A task is done when its checkbox is ticked. A track is done when every
task is. See `DESIGN.md` for the full data model.

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
driver claim <track> <slug> --max-turns <N>     start a claim
driver gate                                      stop-hook callback (do not call directly)
driver release                                   end the current claim
driver claim-status                              show the current claim
driver init-hook                                 print the settings.json snippet
```

After installing the hook (via `driver init-hook` + paste, or by
adding the snippet to `~/.claude/settings.json` directly), each agent
turn ends with `driver gate` running. If a claim is active and the
task isn't ticked or blocked, gate exits 2 — Claude Code interprets
this as "keep working." The claim auto-releases when the task is
ticked (`driver tick`), blocked (`driver block`), or budget exhausted.

## CLI commands

```
driver status                                show all tracks + next runnable
driver next [<track>] [--json]               next runnable task
driver runnable [<track>] [--json]           all currently-runnable tasks
driver tasks [<track>]                       list tasks with status
driver blocked [<track>]                     list blocked tasks + questions
driver tick <track> <slug>                   mark task done
driver untick <track> <slug>                 mark task open again
driver block <track> <slug> "<question>"     write <slug>_blocked.md
driver unblock <track> <slug>                remove <slug>_blocked.md
driver rename <track> <old-slug> <new-slug>  rename a task (updates deps)
driver close <track>                         flip tracks.md to [x] if all done
driver decisions <track>                     print decisions.md

driver claim <track> <slug> --max-turns N    start a claim (Stop hook gate)
driver release                               end the current claim
driver claim-status                          show the current claim
driver gate                                  stop-hook callback
driver init-hook                             print settings.json snippet
driver doctor                                verify PATH, hook, project layout
driver stats [<track>]                       estimate-vs-actual from .history.jsonl

driver ask <track> <slug> "<question>" [--rule <name>] [--context "..."]
                                             log an open question for batched user review
driver questions [<track>]                   list all open questions (answered + unanswered)
driver answer <track> <slug> <Q#> "<answer>" record a decision on question Q#
```

`driver tick` records `{estimate, actual_turns}` to
`driver/.history.jsonl` when the ticked task matches the active claim,
and releases the claim. `driver stats` reads that file. Both files are
gitignored — they're per-developer runtime state.

Run from anywhere inside a project — `driver` walks up looking for
`driver/tracks.md`.

## Skills

| Slash command | What it does |
|---|---|
| `/driver:new` | Asks for slug + summary + task outline; scaffolds files; commits |
| `/driver:status` | Reads `driver/tracks.md` and reports |
| `/driver:next` | Drafts a `/goal` for the next runnable task (paste-then-fire mode) |
| `/driver:do` | Runs the next runnable task end-to-end (Stop-hook safety net) |
| `/driver:go` | Runs *all* runnable tasks until the track is done or blocked |
| `/driver:close` | Closes a track when all its tasks are ticked |

## Principles + the autonomy floor

Driver's autonomy rubric has two layers:

- **Mechanical floor**: optional `driver/principles.md` lists rules with
  file globs. If a task's diff (since claim start) touches a triggered
  file, `driver tick` refuses unless an `driver ask --rule <name>`
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
