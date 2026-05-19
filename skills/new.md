Create a new Driver track. This command creates files and commits, then stops. Do NOT implement any task — that's what `/driver:next` + `/goal` (or `/driver:go`) are for.

## Inputs (ask via `AskUserQuestion`, one prompt with 2–3 questions)

- **Slug** (kebab-case, ~30 chars max). Becomes part of the track id `<YYYYMMDD>-<slug>`.
- **One-line summary** (~100 chars). Goes in the registry bullet.
- **Task outline**: a comma-separated list of task slugs (e.g. `imperatives, possessives, attributive-adjectives, verify`). Optional — if empty, default to a single `placeholder-task`.

## Steps

1. Confirm a git repo (`git rev-parse --git-dir`). If not, refuse: "Driver expects a git repo. Run `git init` first."
2. Compute `track_id = <YYYYMMDD>-<slug>` using today's date in the local timezone.
3. Create `driver/tracks/<track_id>/plan.md` with this skeleton:
   ```markdown
   # <title from summary, capitalised>

   <2–3 sentence problem statement — leave a TODO placeholder>

   - [ ] **<task-slug-1>** (~?? turns)
     <fill in>

   - [ ] **<task-slug-2>** (~?? turns)
     <fill in>
   ```
   One task per slug in the outline. If the outline was empty, emit a single `**placeholder-task**`.
4. If `driver/tracks.md` does not exist, create it with `# Driver tracks\n\n`.
5. Append to `driver/tracks.md`, after any existing bullets (oldest-first):
   ```
   - [ ] [<track_id>](./tracks/<track_id>/plan.md) — <summary>
   ```
6. `git add driver/` and commit with `driver: open track <track_id>`. Use the project's Co-Authored-By footer if its CLAUDE.md prefers one.
7. Print a one-line success with the path to `plan.md`. Tell the user to flesh out descriptions and add `[depends: …]` annotations where needed, then run `/driver:next` (or `/driver:go`).

**Stop after the commit.** Do not implement, do not draft a goal, do not run tests.
