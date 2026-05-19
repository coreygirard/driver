Scaffold a new Driver project (or add the first tasks if one already exists). Creates `driver/plan.md` and commits, then stops. Do NOT implement any task — that's what `/driver:do` / `/driver:go` are for.

## Inputs — you MUST ask the user via `AskUserQuestion`

This skill captures the user's planning intent. **Always ask via `AskUserQuestion` before scaffolding** — do not auto-propose a title or task list from prior conversation context, even when the user has set an autonomous / no-clarifying-questions flag for the session. That flag exists to prevent pauses mid-implementation; choosing *what to plan next* is the exact decision this skill exists to capture, so it overrides the flag here. If you have suggestions, surface them as the options in the `AskUserQuestion` call, not as a pre-baked commit.

Ask in one prompt:

- **One-line title** (~100 chars). Becomes the `# Heading` of `plan.md`.
- **Task outline**: a comma-separated list of task slugs (e.g. `imperatives, possessives, attributive-adjectives, verify`). Optional — if empty, emit a single `placeholder-task`.

## Steps

1. Confirm a git repo (`git rev-parse --git-dir`). If not, refuse: "Driver expects a git repo. Run `git init` first."

2. If `driver/plan.md` already exists, **append** the new task bullets to it under a `## <date> — <title>` section header. Otherwise create the file with this skeleton:
   ```markdown
   # <title>

   <2–3 sentence problem statement — leave a TODO placeholder>

   - [ ] **<task-slug-1>** (~?? turns)
     <fill in>

   - [ ] **<task-slug-2>** (~?? turns)
     <fill in>
   ```
   One task per slug in the outline. If the outline was empty, emit a single `**placeholder-task**`.

3. `git add driver/plan.md` and commit with `driver: scaffold plan` (or `driver: add tasks <slug1>, <slug2>, ...` for an append). Use the project's Co-Authored-By footer if its CLAUDE.md prefers one.

4. Print a one-line success with the path to `plan.md`. Tell the user to flesh out descriptions and add `[depends: …]` annotations where needed, then run `/driver:do` (or `/driver:go`).

**Stop after the commit.** Do not implement, do not draft a goal, do not run tests. Driver is a single evolving `plan.md` — there's no per-track scaffolding.
