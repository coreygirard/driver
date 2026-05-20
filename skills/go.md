Run all runnable tasks end-to-end, autonomously. Loop: claim → implement → tick (or stage as "answered later") → check `driver next` → repeat. Goal is "do everything Driver can do without input from me, then batch all required questions for one review."

This is for "burn down the plan while I'm away" mode.

**Optional argument**: free-text guidance like `/driver:go "continue building parity"` or `/driver:go "focus on auth flows"`. When the project has a `driver/spec.md`, the guidance shapes which unmet spec bullets get synthesized into plan tasks at the start of this run. Without guidance, all unmet bullets are candidates.

**Optional flag**: `--dry-run` (anywhere in the argument string) prints the synthesis plan and stops — no claim, no implementation, no plan.md edits. Use it to sanity-check the decomposition before burning turns.

## Prerequisite check

Run `driver doctor` once at the top. If it exits non-zero, surface the ✗ items to the user and stop. Same recovery as `/driver:do`.

## Session re-entry

Run `driver status` once at the top. It prints the plan summary, active claim, recent ticks, open questions, and spec status (if `spec.md` exists). If there's an active claim or unanswered questions left over from a prior (possibly compacted) session, address those before claiming anything new.

If `driver/principles.md` exists, read it once to know which file changes will trigger the floor.

## Phase 0 — Spec-driven synthesis (if `driver/spec.md` exists)

If the project has a `driver/spec.md`, run this phase before entering the loop. **Skip if the file is absent — the project is plan-only.**

0. Run `driver spec check`. If it exits non-zero, surface the failing bullets to the user and stop — synthesis quality is bounded by spec specificity, and a failing lint means the agent will either produce stub tasks or drown in `driver ask` questions. The user should rewrite the bullets via `/driver:spec` or tag them `[narrative]`. (If the user *really* wants to proceed past a failing lint, they can re-run with `--force-spec`; treat that as an explicit override and note it in the synthesis summary.)

1. Run `driver spec gap --json`. The output is a JSON array of unmet bullets `[{hash, name, body}, …]`.

2. Read `driver/spec.md` for full context (the bodies in `gap` are normalized; the file may have section headers and prose that inform priority).

3. Decide which unmet bullets to address this run:
   - With guidance (`/driver:go "<guidance>"`): pick bullets that match the guidance. State briefly which you picked and which you deferred and why.
   - Without guidance: pick all unmet bullets unless one is clearly out of scope for this iteration.

4. **Decompose each chosen bullet into tasks.** A bullet describes one *commitment*; a task describes one *commit-worthy slice* of work that satisfies part or all of the commitment. The mapping is not 1:1 in either direction:

   - **One bullet → many tasks** when the bullet enumerates or implies enumeration ("renders the 40 CSS properties listed below"), describes a capability with multiple independent sub-capabilities, or would take more than ~30 turns to close in one go. Default to *one task per atomic capability the bullet implies* — one task per CSS property, one task per table type, one task per output format. All N tasks share the same `[spec: <hash>]`. Use `[depends: <slug>]` to order them when one task's output is another's input (typically: infrastructure first, then the slices that consume it).
   - **One task → many bullets** when a single slice naturally closes multiple commitments together. Annotate with `[spec: a3f2b1, b7c2d8]`. Don't force this — only do it when the slices truly overlap.
   - **One bullet → one task** is the default for narrow, atomic commitments ("audit log records every login attempt") — don't manufacture decomposition for its own sake.

   Heuristics for *when to split*:
   - The bullet contains a list ("for each X"), a count ("40 properties"), or a cross-product ("every combination of A and B"). Split.
   - The bullet's natural implementation has a clear infrastructure-then-uses shape (the oracle pipeline before per-feature tests). Split into the infrastructure task and the use-site tasks, with `[depends:]` linking them.
   - Closing the bullet in one task would exceed ~30 turns. Split.

   Heuristics for *when not to split*:
   - The bullet is one tight, narrow commitment that one PR could close.
   - The "split" would just be ceremony — the sub-pieces share so much code that doing them together is cheaper than ticking three artificial slugs.
   - You can't name a falsifiable test boundary between the slices. If the only thing separating slice A from slice B is "I'll do A first, then B," that's a TaskCreate sub-step inside one Driver task, not two tasks.

   When in doubt, **err granular**. Recovery is per-task: a single slice that hits an architectural question halts only itself, not the whole bullet's worth of work. Granular tasks also give `driver stats` real calibration data per slice.

5. Append each task to `driver/plan.md` with this shape:
   ```markdown
   - [ ] **<slug>** (~K turns) [spec: <hash>] [depends: <other-slug>]
     <one-paragraph description, written so an agent firing /goal can act on it
     without re-reading the spec bullet. Reference the bullet by name, not just
     by hash, so the description stands alone.>
   ```
   - Slug: derive from the slice, not from the bullet — `border-em-units`, not `edges` (when the bullet is about all edge units). Slugs must be unique within `plan.md`.
   - Estimate: your best guess in turns. Don't agonize — `driver stats` will calibrate.
   - `[spec: <hash>]`: required for every synthesized task. The verifiable link back to the bullet.
   - `[depends: <slug>]`: use when ordering matters (infrastructure-before-uses, or cross-bullet dependencies). Omit otherwise; most tasks have none.

6. **State the synthesis briefly to the user** before committing: "I'm synthesizing N tasks from M unmet bullets — [one-line summary]. Closing without input." Don't ask for approval; the spec was the approval. Do this so the user can interrupt if the decomposition obviously went wrong.

   **If invoked with `--dry-run`**: instead of writing to plan.md, print the proposed task block (the markdown bullets you would have appended) and stop. Do not commit, do not claim, do not enter the loop. End with: "dry-run complete — re-run /driver:go (no --dry-run) to commit and execute, or adjust the spec first."

7. Commit the appended tasks: `git add driver/plan.md && git commit -m "driver: synthesize <N> tasks from spec — <one-line summary>"`.

8. **Then enter the loop below.** The synthesized tasks now appear in `driver next`.

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

7. **If `driver gate` releases the claim with a "scope signal" message** (new questions per turn ≥ ~1/3): the spec is under-specifying this task, not just hard. *Do not* re-claim and answer-spew. Note the slug as "scope-thin" in the final report, leave the questions for the walkthrough, and continue the outer loop on a different runnable task. The user will see the scope-thin slugs surfaced at the end and can extend the spec via `/driver:spec`.

## Decision logging — what to ask, what to just decide

The high-signal interaction with the user belongs in `/driver:spec`, not here. Every `driver ask` you fire during a `/driver:go` run should be a decision the spec genuinely couldn't have anticipated.

**DO ask** (via `driver ask`, optionally `--rule` if a principles rule applies):
- Cross-cutting architecture the spec doesn't commit to (the z-index oracle question is the gold standard — affects every paint-order test, not just z-index).
- Lossy approximations where "correct enough" is genuinely the user's call.
- Spec ambiguity — multiple reasonable implementations satisfy the bullet wording. The user will refine `spec.md` (via `/driver:spec`) or answer.
- Anything that would make a user a month from now ask "why that way?"

**DO NOT ask** about:
- **Which task to do next.** `driver next` already chose. Don't second-guess it; don't ask the user to pick from runnable tasks. The order rarely carries signal in a pile-of-slices project.
- **Naming, internal helpers, fixture content within established patterns, library trade-offs without lock-in.** Just decide. The commit message carries rationale if it's non-obvious. The user reviews via `git log` / `git diff` after the run.
- **Estimates, budgets, whether to add tests.** These have defaults; use them.

A self-check: *"If I ask this question, will the answer change anything beyond this one task?"* If no — decide. If yes — ask.

The point of front-loading `/driver:spec` is that the run that follows should be lights-out. If you find yourself reaching for `driver ask` more than once per ~5 tasks, the spec is too thin — surface that to the user in the final report rather than logging more questions.

## Final phase: walk through open questions with the user

When the loop exits with open questions still staged, **don't print-and-stop.** Switch into a conversational walkthrough.

1. State a brief summary first: "I ran N tasks, completed X, staged Y. There are Q open questions across [list of slugs]. Let's go through them one at a time."

2. For each open question (run `driver questions` to enumerate):
   - State the question (one or two sentences).
   - State the trade-off, alternatives considered, and your recommendation.
   - Pause. The user may discuss, push back, or want more detail. Respond conversationally.
   - When the user decides, record via `driver answer <slug> <Q#> "<decision>"`.
   - **If the decision is precedent-setting** — it would change how *future* tasks are built, not just this one — also pass `--promote "<bullet body>"` to append it as a new spec.md bullet under `## Architecture`. Phrase the body as a falsifiable commitment, not a Q-and-A transcript. Example: `driver answer z-index 1 "elementsFromPoint via paint_order_at" --promote "**paint-order-oracle** Paint-order tests use a dedicated pipeline (\`paint_order_at(x, y)\` + Chromium's \`document.elementsFromPoint\`) producing a separate \`oracle-paint-order-v1\` snapshot; canonical for any z-index, overflow, or stacking-context test."`. The new bullet's hash will print; future synthesis runs will treat it as a met-by-this-task commitment instead of re-asking.
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
