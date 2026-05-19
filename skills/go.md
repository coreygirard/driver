Run all runnable tasks of a Driver track end-to-end, autonomously. Loop: claim → implement → tick (or stage as "answered later") → check `driver next` → repeat. Goal is "do everything Driver can do without input from me, then batch all required questions for one review."

This is for "run this whole track while I'm away" mode.

## Prerequisite check

Run `driver doctor` once at the top. If it exits non-zero, surface the ✗ items to the user and stop. Same recovery as `/driver:do`.

If `driver/principles.md` exists, read it once to know which file changes will trigger the floor.

## Inputs

Optional positional argument: `<track_id>`. If omitted, picks the most recently modified open track.

## Loop

Repeat until no further progress is possible:

1. Run `driver next [<track_id>] --json`.
   - If "no runnable tasks": check `driver questions [<track_id>]` and `driver blocked [<track_id>]`. If there are unanswered questions or blocks, jump to the **Final report** below. Otherwise the track is complete: `driver close <track_id>` and final report.

2. Follow `/driver:do`'s steps for that one task (design doc if needed, claim, implement, test, commit, tick).

3. **If `driver tick` refuses with "unanswered question(s)":** that task is *staged* — work committed but task remains open. Release the claim (`driver release`) and loop back to step 1 — pick a different runnable task. Keep going until nothing else can be advanced.

4. **If `driver tick` refuses with "principles rule tripped":** run the suggested `driver ask --rule <name>` command. Then either keep working on parts of the task that don't depend on the answer, or — if blocked on the answer — release the claim and loop.

5. **If a task gets `driver block`'d** (fully stuck, not just an open question): release and loop. Downstream tasks that depend on this one are now unreachable; that's fine — the DAG handles it.

6. **If two consecutive tasks fail tests:** stop the loop and final-report. Something structural is wrong.

## Decision logging

For genuinely reversible calls (naming, internal helpers, fixture choices), append a one-liner to `driver/tracks/<track_id>/decisions.md`. The user reviews after the run.

For anything you'd want the user to weigh in on, use `driver ask` (with `--rule` if a principles rule applies, without `--rule` for self-classified asks). These are the questions surfaced in the final report.

## Final report

When the loop exits, gather and print everything the user needs to resume:

```
/driver:go summary — <track_id>

Completed:   N tasks (~total turns spent)
  - <slug>   <commit>   (<actual>/<budget> turns)
  - ...

Staged (work committed, awaiting answers): M tasks
  - <slug>   <K open question(s)>

Blocked (fully stuck): L tasks
  - <slug>   <reason>

Decisions logged: D  → driver/tracks/<track_id>/decisions.md

Open questions: Q (run `driver questions` for full list):

  [<track>/<slug>] Q1 (rule=<name>): <question>
    <context>
    Answer by editing the file: driver/tracks/.../{slug}_questions.md
    (replace `_pending_` with your decision)

  [<track>/<slug>] Q2 ...
```

When the user has answered the questions, they re-run `/driver:go` and the staged tasks become runnable again — agent picks up where it left off.

**Do not modify tracks other than the named one. Do not skip tests. Do not tick a task whose tests don't pass. Do not invent answers to your own questions.**
