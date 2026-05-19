# driver

A minimal `/goal`-native track planner for Claude Code.

`driver` is the smaller cousin of frameworks like Conductor: just enough
structure to drive multi-phase work over several sessions without losing
context, and nothing else.

It is two pieces:

- **`cli/`** — a small Rust binary (`driver`) that handles all mechanical
  operations on a project's `driver/` directory: parsing plans, ticking
  checkboxes, listing the next phase, closing tracks.
- **`skills/`** — Claude Code slash commands (`/driver:new`, `/driver:status`,
  `/driver:next`, `/driver:close`) that wrap the CLI with LLM-judgment
  features for drafting `/goal` prompts and scaffolding new tracks.

Either piece can be used without the other. The CLI is for terminals, git
hooks, and CI. The skills are for sessions where you want help drafting
goal prompts.

## Per-project layout

When you run `/driver:new` (or create the files by hand) in a project:

```
<project>/
  driver/
    tracks.md                   ← registry; one bullet per track
    tracks/
      <YYYYMMDD>-<slug>/
        plan.md                 ← required
        spec.md                 ← optional; the "why"
        phaseN_design.md        ← optional; per-phase design notes
        decisions.md            ← optional; appended by `/driver:go` runs
        blocked.md              ← optional; present when escalating
```

Plans look like:

```markdown
# Track name

2–3 sentences: what we're building and why.

## Phase 1: Imperatives (~10 turns)

- [ ] Add EN imperative parser
- [ ] Add FR imperative parser

## Phase 2: Possessives (~25 turns)

- [ ] ...
```

A phase is complete when all its bullets are `[x]`. A track is complete
when all its phases are.

## Install

```bash
# CLI
cargo install --path cli

# Skills (symlink into Claude Code commands directory)
ln -sf "$(pwd)/skills" ~/.claude/commands/driver
```

The CLI is then available as `driver` on PATH. Skills are `/driver:new`,
`/driver:status`, `/driver:next`, `/driver:close`.

## CLI commands

```
driver status                         show all tracks and their next phase
driver next [<track-id>] [--json]     print next unchecked phase
driver tick <track> <phase>           tick all bullets of a phase
driver tick-bullet <track> <p> <b>    tick one bullet (1-indexed)
driver close <track>                  flip tracks.md to [x] if all phases done
driver block <track> "<question>"     write blocked.md
driver unblock <track>                remove blocked.md
driver decisions <track>              print decisions.md
```

Run from anywhere inside a project — `driver` walks up looking for
`driver/tracks.md`.

## Skills

| Slash command | What it does |
|---|---|
| `/driver:new` | Asks for a slug + summary + phase outline; scaffolds files; commits |
| `/driver:status` | Reads `driver/tracks.md` and reports |
| `/driver:next` | Drafts a `/goal` for the next unchecked phase |
| `/driver:close` | Closes a track when all its phases are ticked |

## Design choices

- **One plan file is the source of truth.** No `metadata.json`, no per-track
  `index.md`, no `notes.md`.
- **Two states: open and done.** No `[~]` "in progress" — the open phase is
  always the first unchecked one.
- **Track IDs are `<YYYYMMDD>-<slug>`.** No `track_` prefix; the directory
  location is enough.
- **CLI does mechanical things; skills do LLM-judgment things.** The CLI
  doesn't decide what's "design-worthy" — that's the LLM's job.
- **Agents don't need to know it's Driver.** A `plan.md` is readable on its
  own; no resolution protocol or context file references are required.

See the per-project `driver/principles.md` (optional) for project-specific
rules of thumb that apply to every track.

## License

MIT.
