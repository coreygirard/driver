Run all runnable tasks of a Driver track end-to-end, autonomously. Equivalent to `/driver:do` in a loop: claim → implement → tick → check `driver next` again → repeat. Stops when no runnable tasks remain (track is done or all remaining tasks are blocked).

This is for "go run this whole track while I'm away" mode.

## Prerequisite check

Run `driver doctor` once at the top. If it exits non-zero, surface the ✗ items to the user and stop. Same recovery instructions as `/driver:do`.

## Inputs

Optional positional argument: `<track_id>`. If omitted, picks the most recently modified open track.

## Steps

Loop until exit:

1. Run `driver next [<track_id>] --json`. If it errors with "no runnable tasks":
   - Check `driver blocked [<track_id>]`. If any task is blocked, print the questions and stop.
   - Otherwise the track is complete: run `driver close <track_id>` and stop.

2. Follow the steps from `/driver:do` for this one task:
   a. Write `<slug>_design.md` if the task needs design (per `/driver:do` heuristics) and none exists.
   b. `driver claim <track_id> <slug> --max-turns <ceil(estimate * 1.3 / 5) * 5>`.
   c. Implement the task. Stop hook keeps you on it.
   d. Run the project's test suite. Reject the tick if anything fails.
   e. Commit. `driver tick <track_id> <slug>`.

3. Print a one-line summary for the completed task: `Completed <slug> — <commit-hash>.`

4. Loop back to step 1.

## What stops the loop

- All tasks done → `driver close` and exit with a final summary (commits made, total turns spent).
- A task gets blocked → print the blocking question and exit. The user can resolve it later and re-run `/driver:go` to pick up where we left off.
- Turn budget exhausted on a single task → the gate auto-releases. Print the partial state and exit without ticking that task. The user can re-run after the issue is understood.
- Two consecutive tasks fail tests → exit with a summary; something is probably wrong.

## Decision logging

Across the run, append a short entry to `driver/tracks/<track_id>/decisions.md` for each non-obvious reversible decision you made. One line each — slug, what you decided, one-clause why. The user reviews this after the run; they can `git revert` decisions they disagree with.

## Final report

When the loop exits, print:

```
/driver:go summary

  track: <track_id>
  duration: <wall time>
  tasks completed: N
  blocked: <slug>   (with the question, if any)
  decisions logged: M  — see driver/tracks/<track_id>/decisions.md
```

**Do not modify tracks other than the named one. Do not skip tests. Do not tick a task whose tests don't pass.**
