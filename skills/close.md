Close a Driver track. Verifies that every task is ticked, then flips the registry line in `driver/tracks.md` to `[x]` and commits.

## Inputs

Required positional argument: `<track_id>`. Refuse if missing.

## Steps

1. Run `driver close <track_id>`. If it errors, print the error verbatim and stop (don't commit anything).
2. If it succeeded, `git add driver/tracks.md` and commit with `driver: close track <track_id>`. Use the project's Co-Authored-By footer if its CLAUDE.md prefers one.
3. Print one line: `Closed <track_id>.`

**Do not modify any plan.md or task design docs. The CLI already touched tracks.md; this skill just commits.**
