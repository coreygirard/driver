Bootstrap a new Driver project. **Spec-first**: the first thing this skill does is run the spec interrogation (via `/driver:spec`), because the high-signal interaction belongs upfront. Plan tasks are derived from spec by `/driver:go`, not authored here.

This skill never implements tasks. Use `/driver:do` or `/driver:go` for that.

## What this skill does (and what it doesn't)

The old version of this skill asked for a task outline and scaffolded `plan.md` directly. That was the wrong shape: it shifted the user into queue-manager mode (nominating tasks one by one) and skipped the upfront contract. The new version:

1. Scaffolds `spec.md` first by running the `/driver:spec` interrogation (target / shape / architecture / out-of-scope).
2. Scaffolds an empty `plan.md` with just the title — no task bullets.
3. Stops, pointing the user at `/driver:go` to synthesize the initial plan from the spec.

If the user *insists* on the old plan-only flow ("I just want a task list, no spec"), allow it but state plainly that the project will require more along-the-way decisions and that `/driver:go` will work better once a spec exists.

## Steps

1. Confirm a git repo (`git rev-parse --git-dir`). If not, refuse: "Driver expects a git repo. Run `git init` first."

2. If `driver/plan.md` or `driver/spec.md` already exists, this isn't a new project. Surface the existing state via `driver status` and route the user appropriately:
   - Want to extend the contract? → `/driver:spec`
   - Want to do work? → `/driver:do` or `/driver:go`
   - Want to add an *unspecified* task (rare; needs justification)? Allow, but as a plain plan.md append with no `[spec: ...]` annotation, and warn that this task won't carry traceability back to the spec.

3. For a fresh project, ask via `AskUserQuestion`:
   - **Project name** (becomes the `# Heading` of both spec.md and plan.md).
   - **One-line description** (becomes the preamble of both files).

4. **Run the `/driver:spec` interrogation.** Walk all four categories (target / shape / architecture / out-of-scope). Do not skip this even when the user has set an autonomous / no-clarifying-questions flag — choosing what the project commits to being is the *exact* decision this skill exists to capture; the flag does not apply here.

5. Write `driver/spec.md` with the title, preamble, and the four `## Section` headers (with bullets where the user answered, `TBD` placeholders where they punted).

6. Write `driver/plan.md` with just:
   ```markdown
   # <Project name>

   <preamble>
   ```
   No task bullets. Plan is derived from spec.

7. Stage both files and commit with `driver: scaffold project — <name>`. Use the project's Co-Authored-By footer if its CLAUDE.md prefers one.

8. Print:
   ```
   Driver project scaffolded.
   - spec.md: N bullets across {Target, Shape, Architecture, Out of scope}
   - plan.md: title only (tasks will be synthesized from spec)

   Next: /driver:go to synthesize initial plan tasks from the unmet spec bullets.
   ```

**Stop after the commit.** Do not implement, do not draft a goal, do not run `/driver:go` automatically.

## Why spec-first

The data from real Driver usage: a single project shipped 18 narrow tasks in one day, with the user nominating each next task. Of those 18 tasks, only one logged a question — and that one question (about oracle architecture) was a *cross-cutting* decision that should have been part of the spec from the start. The other 17 tasks were the agent grinding through obvious work that didn't need user input on "which next."

Front-loading the cross-cutting decisions into `spec.md` turns the rest of the run into lights-out work. That's the trade `/driver:new` is making.
