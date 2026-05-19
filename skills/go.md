Run all runnable tasks end-to-end, autonomously. Loop: claim → implement → tick (or stage as "answered later") → check `driver next` → repeat. Goal is "do everything Driver can do without input from me, then batch all required questions for one review."

This is for "burn down the plan while I'm away" mode.

## Prerequisite check

Run `driver doctor` once at the top. If it exits non-zero, surface the ✗ items to the user and stop. Same recovery as `/driver:do`.

## Session re-entry

Run `driver status` once at the top. It prints the plan summary, active claim, recent ticks, and open questions. If there's an active claim or unanswered questions left over from a prior (possibly compacted) session, address those before claiming anything new.

If `driver/principles.md` exists, read it once to know which file changes will trigger the floor.

## Loop

Repeat until no further progress is possible:

1. Run `driver next --json`.
   - If "no runnable tasks": the `== open questions ==` section of `driver status` (already printed above) shows what's blocking things. If anything is unanswered, jump to the **Final report** below. Otherwise the plan is complete — final report.

2. Follow `/driver:do`'s steps for that one task (design doc if needed, claim, implement, test, commit, tick).

3. **If `driver tick` refuses with "unanswered question(s)":** that task is *staged* — work committed but task remains open. Release the claim (`driver release`) and loop back to step 1 — pick a different runnable task. Keep going until nothing else can be advanced.

4. **If `driver tick` refuses with "principles rule tripped":** run the suggested `driver ask --rule <name>` command. Then either keep working on parts of the task that don't depend on the answer, or — if blocked on the answer — release the claim and loop.

5. **If a task is fully stuck** (not just an open question — you genuinely can't make further progress): `driver ask <slug> --halt "<question>"`. That logs the question with a halt marker, releases the active claim, and makes dependents transitively unreachable until the question is answered. Loop on.

6. **If two consecutive tasks fail tests:** stop the loop and final-report. Something structural is wrong.

## Decision logging

For genuinely reversible calls (naming, internal helpers, fixture choices), just make them — let the commit message carry the rationale if it's non-obvious. The user reviews via `git log` / `git diff` after the run.

For anything you'd want the user to weigh in on, use `driver ask` (with `--rule` if a principles rule applies, without `--rule` for self-classified asks). These are the questions surfaced in the final report.

## Final phase: walk through open questions with the user

When the loop exits with open questions still staged, **don't print-and-stop.** Switch into a conversational walkthrough.

1. State a brief summary first: "I ran N tasks, completed X, staged Y. There are Q open questions across [list of slugs]. Let's go through them one at a time."

2. For each open question (run `driver questions` to enumerate):
   - State the question (one or two sentences).
   - State the trade-off, alternatives considered, and your recommendation.
   - Pause. The user may discuss, push back, or want more detail. Respond conversationally.
   - When the user decides, record via `driver answer <slug> <Q#> "<decision>"`.
   - Move to the next question.

3. Once all questions are answered (or the user wants to leave some for later), offer to resume: "All answered — want me to continue running `/driver:go` from where we left off?" If yes, restart the loop from step 1 of the outer Loop.

Order the questions by importance: mechanical-floor (rule-tagged) questions first, since they tend to be more foundational than self-classified ones. Within each group, walk in Q-number order.

If the user wants to skip a question ("come back to that later"), respect it. That task stays staged; the others can still resume.

## Completion report

After the conversation phase, if everything resolved and the loop ran to true completion:

```
/driver:go done

Completed:   N tasks (~total turns spent)
  - <slug>   <commit>   (<actual>/<budget> turns)

Questions answered:  Q (all resolved during this run)
```

If there are still unanswered questions or blocks at the end:

```
/driver:go paused

Completed:   N tasks
Staged (still open):
  - <slug>   <K unanswered question(s)>

Re-run /driver:go when you want to walk through the remaining questions.
```

**Do not skip tests. Do not tick a task whose tests don't pass. Do not invent answers to your own questions.**
