Draft a `/goal` for the next runnable task of a Driver track. Read-only — never edit files, never commit, never invoke `/goal` yourself. Output the goal text for the user to paste manually.

## Inputs

Optional positional argument: `<track_id>`. If omitted, the CLI picks the open track whose plan.md was most recently modified.

## Steps

1. Run `driver next [<track_id>] --json` to get the next runnable task as JSON. Capture: `slug`, `estimate`, `depends`, `description`.
2. If the CLI errors with "no runnable tasks" → check `driver blocked [<track_id>]`. If anything is blocked, print the blocking questions and tell the user to resolve them and re-run. Stop without drafting.
3. Check whether `driver/tracks/<track_id>/<slug>_design.md` exists. If it does NOT, and the task description mentions any of: "schema", "CoreIr", "Predicate", "Entity", "lexicon shape", "public API", "design pass first", "design pass" — recommend writing the design doc first, but do not refuse. Note it in your output.
4. Draft a `/goal` using this skeleton:
   ```
   /goal Implement the task <slug> of driver/tracks/<track_id>/plan.md.

   <verbatim description from the CLI output>

   When the implementation is complete, run `driver tick <track_id> <slug>` to mark the task done.

   Prove by `cargo fmt --check` exit 0 (or the project's analogue), the project's test suite exits 0 with no regressions, and any task-specific verification criteria above. Stop after <estimate * 1.3, rounded to nearest 5> turns. If you encounter a hard-to-reverse design question not answered in the description or `<slug>_design.md`, run `driver block <track_id> <slug> "<question>"` and stop.
   ```

   If the project's CLAUDE.md references a non-Rust toolchain, substitute the appropriate commands. If unsure, leave a `<fill in test command>` placeholder.

5. Print the drafted `/goal` text in a fenced code block. Above it, print a one-line context summary (slug + estimate + dependencies). If step 3 flagged the task, add: "Consider writing `driver/tracks/<track_id>/<slug>_design.md` and committing it before firing this goal."

**Do not invoke `/goal`. Do not modify any files. Do not commit. Just draft and print.**
