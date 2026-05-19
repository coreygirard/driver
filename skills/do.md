Run the next runnable task of a Driver track end-to-end. Claims the task, implements it, ticks when done. The Stop hook (configured via `driver init-hook`) keeps the agent working until the task is finished, blocked, or has open questions that prevent ticking.

## Prerequisite check

Run `driver doctor` once at the top. If it exits non-zero, surface the ✗ items to the user and stop — the typical fixes are (a) put `~/.cargo/bin` on PATH or symlink the binary, (b) run `driver init-hook` and paste into `~/.claude/settings.json`, (c) `cd` into a project with `driver/tracks.md`. After the user fixes the issue (may require a fresh Claude Code session if the hook was just installed), they can re-run `/driver:do`.

If `driver/principles.md` exists, read it once at the top to understand which file changes will require an open question before tick.

## Inputs

Optional positional argument: `<track_id>`. If omitted, the CLI picks the open track whose plan.md was most recently modified.

## Steps

1. Run `driver next [<track_id>] --json`. Capture `track_id`, `slug`, `estimate`, `depends`, `description`. If the CLI reports "no runnable tasks", run `driver questions` + `driver blocked` and report open items to the user; stop.

2. Check whether `driver/tracks/<track_id>/<slug>_design.md` exists. If it doesn't AND the description contains any of: "schema", "CoreIr", "Predicate", "Entity", "design pass first", or any other strong signal that the task needs upfront design — write the design doc as the first step. Commit it as one commit before proceeding with implementation.

3. Run `driver claim <track_id> <slug> --max-turns <ceil(estimate * 1.3 / 5) * 5>`. If claim fails (an existing claim is active), refuse with the error message.

4. Implement the task. Use TaskCreate to track sub-steps if helpful. The Stop hook keeps you on this task until either:
   - You run `driver tick <track_id> <slug>` (task complete and committed; tick enforces the principles floor + answered-status — see Autonomy section below).
   - You run `driver block <track_id> <slug> "<question>"` (you are fully stuck — no progress possible on this task without input).
   - The turn budget runs out (gate auto-releases and warns).

5. Before ticking: run the project's test suite (e.g. `cargo fmt --check && cargo test` for Rust; check `CLAUDE.md` or `README.md` for the right commands). All tests must pass with no regressions.

6. Commit the implementation with a clear message that includes the task slug. Then `driver tick <track_id> <slug>` (writes `.history.jsonl`, releases the active claim, enforces gates).

   If tick refuses with a "principles rule tripped" error, run the suggested `driver ask --rule <name>` command and continue.

   If tick refuses with "unanswered question(s)", **walk the user through them one at a time.** Don't dump the list and stop. For each open question:
   - State the question (one or two sentences, not the full file contents).
   - State the context the user needs to weigh in: what's the trade-off, what alternatives were considered, what's your recommendation.
   - Pause for the user to discuss. They may push back, ask for alternatives, or want more detail. Respond conversationally.
   - When the user has decided, record the answer with `driver answer <track_id> <slug> <Q#> "<decision>"`. The decision can be terse — the conversation captures the reasoning, the file records the verdict.
   - Move to the next question.

   After all questions are answered, re-run `driver tick`. It should succeed (assuming no new floor trips). Proceed to step 7.

   If the user wants to stop mid-walkthrough (e.g. "I need to think about Q2 — answer Q1 and Q3, leave Q2 for later"), respect that. The task remains staged with Q2 pending; the user re-runs `/driver:do` when ready.

7. Print a multi-line summary using the data you already have:
   - `Completed <slug> — <commit-hash>`
   - `Files touched: <git diff --stat | tail -1>`
   - `Tests added: <count of new test fns, if any>`
   - `Fixtures added: <count of new entries in testdata/, if any>`
   - `Turns: <actual>/<budget> (est ~<estimate>t, ratio <r>)` — copy the budget line that `driver tick` printed
   - `Decisions logged: <count of lines added to decisions.md>` if you touched it

   Keep it terse — six lines max.

## Autonomy rubric (mechanical floor + self-classification ceiling)

There are two layers of escalation. They compose: the floor is leak-proof, the ceiling is judgment-driven.

**Mechanical floor (enforced by `driver tick`).** `driver/principles.md` lists rules. Each rule names a file glob; touching any matching file during a claim makes the rule "tripped." Tick refuses unless you have logged a question with that rule tag:

```
driver ask <track> <slug> --rule <name> "<question>" --context "..."
```

You can't bypass this. If you touch `src/core_ir.rs` (or whatever paths are listed), you must ask.

**Self-classification ceiling (your judgment).** Use `driver ask` *without* `--rule` for anything that:
- locks multiple future tasks into a pattern,
- ships a lossy approximation (where "correct enough" is your call),
- might surprise the user ("they could have wanted this differently"),
- changes how concepts are represented (entity shape, fixture format, modelling conventions).

Examples of self-classified asks from real runs:
- "Store adjective modifiers as concept-ids vs full Entity records?"
- "Approximate DE mixed declension as weak?"
- "48 declension cells vs 72 with mixed paradigm?"

You can keep working on parts of the task that don't depend on the answer. The question waits for the user. If you genuinely can't make further progress, `driver block` (fully stuck) on top.

**What still belongs in `decisions.md`.** Reversible local calls — naming, internal helpers, test fixture content within the patterns the description names, library choices that don't lock long-term. One line each. The user reviews after the run.

**When in doubt: ask.** Asking is cheap (one CLI call, doesn't halt). The user will see all questions batched at the end of `/driver:go`. Over-asking is fine; under-asking is what we're trying to prevent.

**Do not modify other tracks' plan.md, do not skip the test run, do not tick a task whose tests don't pass.**
