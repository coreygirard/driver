Close a Driver track. Verifies that every task is ticked.

## Inputs

Required positional argument: `<track_id>`. Refuse if missing.

## Steps

1. Run `driver close <track_id>`. If it errors, print the error verbatim and stop.
2. Print one line: `Closed <track_id>.`

**Do not modify any plan.md or task design docs.** Done-ness is derived from plan.md task state, so closing a track is a pure verification — no file edits, no commit needed.
