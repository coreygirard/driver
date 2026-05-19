Run the next runnable task end-to-end. Claims the task, implements it, ticks when done. The Stop hook keeps the agent working until the task is finished, halted, or has open questions that prevent ticking.

## Prerequisite check

Run `driver doctor` once at the top. If it exits non-zero, surface the ✗ items to the user and stop — the typical fixes are (a) put `~/.cargo/bin` on PATH or symlink the binary, (b) paste the snippet doctor printed into `~/.claude/settings.json`, (c) `cd` into a project with `driver/plan.md`. After the user fixes the issue (may require a fresh Claude Code session if the hook was just installed), they can re-run `/driver:do`.

## Session re-entry

Run `driver status` once at the top. It prints the plan summary (with the next-runnable task inline), the active claim (from a prior session that may have compacted), recent ticks, and open questions. Read it before deciding what to do — if there's already an active claim or unanswered questions, surface those first rather than blindly running `driver next`.

If `driver/principles.md` exists, read it once at the top to understand which file changes will require an open question before tick.

## Steps

1. Run `driver next --json`. Capture `slug`, `estimate`, `depends`, `description`, and **`spec`**. If the CLI reports "no runnable tasks", the `== open questions ==` section of `driver status` (already printed above) shows what's blocking things — surface those open items to the user and stop.

   **Read the `spec` field.** It is a JSON array of `{hash, name, body}` objects describing the spec commitments this task implements. If non-empty, **the bullet bodies are the binding contract for "done"** — the implementation must satisfy each one, not merely match the prose `description`. The description is the agent-readable framing; the spec bullet bodies are the verifiable target. If a body is `null`, the reference is an orphan (the user edited the spec since the task was written) — surface this to the user before implementing; do not guess what the bullet used to say.

2. Check whether `driver/<slug>_design.md` exists. If it doesn't AND the description contains any of: "schema", "Predicate", "Entity", "design pass first", or any other strong signal that the task needs upfront design — write the design doc as the first step. Commit it as one commit before proceeding with implementation.

3. Run `driver claim <slug>`. The CLI defaults `--max-turns` to `ceil(estimate × 1.3 / 5) × 5` from the task's plan estimate; pass `--max-turns N` only if you have reason to override (e.g., you expect scope growth). If claim fails (an existing claim is active), refuse with the error message.

4. Implement the task. Use TaskCreate to track sub-steps if helpful. The Stop hook keeps you on this task until either:
   - You run `driver tick <slug>` (task complete and committed; tick enforces the principles floor + answered-status — see Autonomy section below).
   - You run `driver ask <slug> --halt "<question>"` (you are fully stuck — no progress possible on this task without input). This releases the claim automatically.
   - The turn budget runs out (gate auto-releases and warns).

5. Before ticking: run the project's test suite (e.g. `cargo fmt --check && cargo test` for Rust; check `CLAUDE.md` or `README.md` for the right commands). All tests must pass with no regressions.

6. Commit the implementation with a clear message that includes the task slug. Then `driver tick <slug>` (writes `.history.jsonl`, releases the active claim, enforces gates).

   If tick refuses with a "principles rule tripped" error, run the suggested `driver ask --rule <name>` command and continue.

   If tick refuses with "unanswered question(s)", **walk the user through them one at a time.** Don't dump the list and stop. For each open question:
   - State the question (one or two sentences, not the full file contents).
   - State the context the user needs to weigh in: what's the trade-off, what alternatives were considered, what's your recommendation.
   - Pause for the user to discuss. They may push back, ask for alternatives, or want more detail. Respond conversationally.
   - When the user has decided, record the answer with `driver answer <slug> <Q#> "<decision>"`. The decision can be terse — the conversation captures the reasoning, the file records the verdict.
   - Move to the next question.

   After all questions are answered, re-run `driver tick`. It should succeed (assuming no new floor trips). Proceed to step 7.

   If the user wants to stop mid-walkthrough (e.g. "I need to think about Q2 — answer Q1 and Q3, leave Q2 for later"), respect that. The task remains staged with Q2 pending; the user re-runs `/driver:do` when ready.

7. Print a multi-line summary using the data you already have:
   - `Completed <slug> — <commit-hash>`
   - `Files touched: <git diff --stat | tail -1>`
   - `Tests added: <count of new test fns, if any>`
   - `Fixtures added: <count of new entries in testdata/, if any>`
   - `Turns: <actual>/<budget> (est ~<estimate>t, ratio <r>)` — copy the budget line that `driver tick` printed
   Keep it terse — six lines max.

## Autonomy rubric (mechanical floor + self-classification ceiling)

There are two layers of escalation. They compose: the floor is leak-proof, the ceiling is judgment-driven.

**Mechanical floor (enforced by `driver tick`).** `driver/principles.md` lists rules. Each rule names a file glob; touching any matching file during a claim makes the rule "tripped." Tick refuses unless you have logged a question with that rule tag:

```
driver ask <slug> --rule <name> "<question>" --context "..."
```

You can't bypass this. If you touch a path on a tripped glob, you must ask.

**Self-classification ceiling (your judgment).** Use `driver ask` *without* `--rule` for anything that:
- locks multiple future tasks into a pattern,
- ships a lossy approximation (where "correct enough" is your call),
- might surprise the user ("they could have wanted this differently"),
- changes how concepts are represented (entity shape, fixture format, modelling conventions).

You can keep working on parts of the task that don't depend on the answer. The question waits for the user. If you genuinely can't make further progress, re-run with `--halt` (`driver ask <slug> --halt "..."`) — that releases the claim and marks the task halted until answered.

**What stays out of `ask` entirely.** Reversible local calls — naming, internal helpers, test fixture content within the patterns the description names, library choices that don't lock long-term — just do them, and let the commit message carry the rationale if it's non-obvious. The user reviews via `git log` / `git diff` after the run.

**When in doubt: ask.** Asking is cheap (one CLI call, doesn't halt). The user will see all questions batched at the end of `/driver:go`. Over-asking is fine; under-asking is what we're trying to prevent.

**Do not skip the test run, do not tick a task whose tests don't pass.**
