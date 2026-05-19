Create a new Driver track. This command creates files and commits, then stops. Do NOT implement any phase work — that's what `/driver:next` + `/goal` are for.

## Inputs (ask via `AskUserQuestion`, one prompt with 2–3 questions)

- **Slug** (kebab-case, ~30 chars max). Will become part of the track id `<YYYYMMDD>-<slug>`.
- **One-line summary** (~100 chars). Goes in the registry bullet.
- **Phase outline**: a comma-separated list like `imperatives, possessives, attributive-adjectives, coordination, relatives, verify`. One phase per item. Optional — if empty, default to a single `Phase 1: <fill in>` placeholder.

## Steps

1. Confirm a git repo (`git rev-parse --git-dir`). If not, refuse with: "Driver expects a git repo. Run `git init` first."
2. Compute `track_id = <YYYYMMDD>-<slug>` using today's date in the local timezone.
3. Create `driver/tracks/<track_id>/plan.md` with this exact skeleton:
   ```markdown
   # <title from summary, capitalised>

   <2–3 sentence problem statement — leave a TODO placeholder for the user to fill in>

   ## Phase 1: <name> (~?? turns)

   - [ ] <fill in>

   ## Phase 2: <name> (~?? turns)

   - [ ] <fill in>
   ```
   One `## Phase N: <name> (~?? turns)` heading per phase from the outline. If the outline was empty, emit a single placeholder phase.
4. If `driver/tracks.md` does not exist, create it with the header:
   ```markdown
   # Driver tracks

   ```
5. Append a bullet to `driver/tracks.md`, *after* any existing bullets (oldest-first ordering):
   ```
   - [ ] [<track_id>](./tracks/<track_id>/plan.md) — <summary>
   ```
6. `git add driver/` and commit with message `driver: open track <track_id>`. Use the standard Co-Authored-By footer the project's CLAUDE.md prefers if one is detected; otherwise omit.
7. Print a one-line success message with the path to `plan.md`. Tell the user to flesh out the spec and phase bullets, then run `/driver:next` when ready to draft the first `/goal`.

**Stop after the commit.** Do not implement, do not draft a goal, do not run tests.
