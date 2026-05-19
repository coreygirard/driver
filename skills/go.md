Run all runnable tasks end-to-end, autonomously. Loop: claim → implement → tick (or stage as "answered later") → check `driver next` → repeat. Goal is "do everything Driver can do without input from me, then batch all required questions for one review."

This is for "burn down the plan while I'm away" mode.

**Optional argument**: free-text guidance like `/driver:go "continue building parity"` or `/driver:go "focus on auth flows"`. When the project has a `driver/spec.md`, the guidance shapes which unmet spec bullets get synthesized into plan tasks at the start of this run. Without guidance, all unmet bullets are candidates.

## Prerequisite check

Run `driver doctor` once at the top. If it exits non-zero, surface the ✗ items to the user and stop. Same recovery as `/driver:do`.

## Session re-entry

Run `driver status` once at the top. It prints the plan summary, active claim, recent ticks, open questions, and spec status (if `spec.md` exists). If there's an active claim or unanswered questions left over from a prior (possibly compacted) session, address those before claiming anything new.

If `driver/principles.md` exists, read it once to know which file changes will trigger the floor.

## Phase 0 — Spec-driven synthesis (if `driver/spec.md` exists)

If the project has a `driver/spec.md`, run this phase before entering the loop. **Skip if the file is absent — the project is plan-only.**

1. Run `driver spec gap --json`. The output is a JSON array of unmet bullets `[{hash, name, body}, …]`.

2. Read `driver/spec.md` for full context (the bodies in `gap` are normalized; the file may have section headers and prose that inform priority).

3. Decide which unmet bullets to address this run:
   - With guidance (`/driver:go "<guidance>"`): pick bullets that match the guidance. State briefly which you picked and which you deferred and why.
   - Without guidance: pick all unmet bullets unless one is clearly out of scope for this iteration.

4. For each chosen bullet, append a task to `driver/plan.md` with the shape:
   ```markdown
   - [ ] **<slug>** (~K turns) [spec: <hash>]
     <one-paragraph description of the work, written so an agent firing /goal can act on it>
   ```
   - Slug: derive from the bullet name if it has one, otherwise from the body. Keep slugs unique within `plan.md`.
   - Estimate: your best guess in turns. Don't agonize — `driver stats` will calibrate later.
   - `[spec: <hash>]`: required. This is the verifiable link back to the spec.
   - Multi-bullet tasks are fine: `[spec: a3f2b1, b7c2d8]` if one task closes both bullets together.
   - Add `[depends: <other-slug>]` if the task needs another task first.

5. Commit the appended tasks: `git add driver/plan.md && git commit -m "driver: synthesize tasks from spec — <one-line summary>"`.

6. **Then enter the loop below.** The synthesized tasks now appear in `driver next`.

If `driver spec gap` reports no unmet bullets *and* `plan.md` has no open tasks: the project is fully built against its current spec. Tell the user and stop. They probably want to run `/driver:spec` to extend the spec.

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

**Spec ambiguity is a question, not a guess.** If an unmet spec bullet is genuinely ambiguous — multiple reasonable implementations satisfy the wording — `driver ask <slug> "<what's ambiguous>"` instead of picking. The user will refine spec.md (via `/driver:spec`) or answer the question.

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
  - <slug>   <commit>   (<actual>/<budget> turns)   [spec: <hash> if applicable]

Spec status: M/N bullets met
Questions answered:  Q (all resolved during this run)
```

If there are still unanswered questions or blocks at the end:

```
/driver:go paused

Completed:   N tasks
Staged (still open):
  - <slug>   <K unanswered question(s)>

Spec status: M/N bullets met, U unmet
Re-run /driver:go when you want to walk through the remaining questions.
```

**Do not skip tests. Do not tick a task whose tests don't pass. Do not invent answers to your own questions. Do not edit spec.md during a run — that's `/driver:spec`'s job.**
