Run the next runnable task of a Driver track end-to-end. Claims the task, implements it, ticks when done. The Stop hook (configured via `driver init-hook`) keeps the agent working until the task is finished or blocked.

## Prerequisite check

Run `driver doctor` once at the top. If it exits non-zero, surface the ✗ items to the user and stop — the typical fixes are (a) put `~/.cargo/bin` on PATH or symlink the binary, (b) run `driver init-hook` and paste into `~/.claude/settings.json`, (c) `cd` into a project with `driver/tracks.md`. After the user fixes the issue (may require a fresh Claude Code session if the hook was just installed), they can re-run `/driver:do`.

## Inputs

Optional positional argument: `<track_id>`. If omitted, the CLI picks the open track whose plan.md was most recently modified.

## Steps

1. Run `driver next [<track_id>] --json`. Capture `track_id`, `slug`, `estimate`, `depends`, `description`. If the CLI reports "no runnable tasks", check `driver blocked` and report any open questions to the user; stop.

2. Check whether `driver/tracks/<track_id>/<slug>_design.md` exists. If it doesn't AND the description contains any of: "schema", "CoreIr", "Predicate", "Entity", "design pass first", or any other strong signal that the task needs upfront design — write the design doc as the first step. Commit it as one commit before proceeding with implementation. The design doc should answer the specific design questions the description names.

3. Run `driver claim <track_id> <slug> --max-turns <ceil(estimate * 1.3 / 5) * 5>`. If claim fails (an existing claim is active), refuse with the error message.

4. Implement the task. Use TaskCreate to track sub-steps if helpful. The Stop hook will keep you on this task until either:
   - You run `driver tick <track_id> <slug>` (task complete and committed).
   - You run `driver block <track_id> <slug> "<specific question>"` (hard-to-reverse design question that needs user input).
   - The turn budget runs out (gate auto-releases and warns).

5. Before ticking: run the project's test suite (e.g. `cargo fmt --check && cargo test` for Rust; check `CLAUDE.md` or `README.md` for the right commands). All tests must pass with no regressions in prior tasks' fixtures.

6. Commit the implementation with a clear message that includes the task slug. Then `driver tick <track_id> <slug>` (this writes a record to `driver/.history.jsonl` and releases the active claim automatically).

7. Print a multi-line summary using the data you already have:
   - `Completed <slug> — <commit-hash>`
   - `Files touched: <git diff --stat | tail -1>`
   - `Tests added: <count of new test fns, if any>`
   - `Fixtures added: <count of new entries in testdata/, if any>`
   - `Turns: <actual>/<budget> (est ~<estimate>t, ratio <r>)` — copy the budget line that `driver tick` printed
   - `Decisions logged: <count of lines added to decisions.md>` if you touched it

   Keep it terse — six lines max. The user reads this to know what to review.

## Autonomy rubric (when to decide vs. escalate)

Decide and proceed (log to `driver/tracks/<track_id>/decisions.md` if non-obvious):
- Naming of variables, helpers, files.
- Internal data structures and algorithms.
- Test fixture content within the patterns the description names.
- Library/dep choices that don't lock long-term.

Escalate via `driver block` (don't guess):
- Public API changes not explicitly authorised in the description.
- Schema-level changes to core IR types when the description doesn't say to.
- Deletions of existing tests or fixtures.
- Architectural choices that affect future tasks.

When in doubt: can a future task undo this with a small diff? If yes, decide. If no, block.

**Do not modify other tracks' plan.md, do not skip the test run, do not tick a task whose tests don't pass.**
