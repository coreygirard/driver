Draft a `/goal` for the next unchecked phase of a Driver track. Read-only — never edit files, never commit, never invoke `/goal` yourself. Output the goal text for the user to paste manually.

## Inputs

Optional positional argument: `<track_id>`. If omitted, pick the open (`[ ]`) track in `driver/tracks.md` whose `plan.md` was most recently modified (`git log -1 --format=%ct -- driver/tracks/<id>/plan.md` per track; pick the largest). If there is exactly one open track, use it without ambiguity.

## Steps

1. If `driver/tracks.md` doesn't exist, refuse with: "No Driver tracks in this project. Run `/driver:new` first."
2. Resolve the track:
   - With a `<track_id>` arg: read `driver/tracks/<track_id>/plan.md`. If missing, refuse.
   - Without an arg: per the heuristic above.
3. Find the **first** phase heading (`^## Phase \d+: ...`) whose body contains at least one `[ ]` bullet. Capture: phase number, phase name, the parenthesised turn estimate if present, and the full set of unchecked + checked bullets in that phase.
4. Check whether a `phaseN_design.md` exists in the same directory (where `N` is the phase number). If it does NOT, and the phase looks design-worthy by these heuristics:
   - The phase touches `src/core_ir.rs`, `src/validation.rs`, or any other schema file.
   - The phase introduces a new IR variant or a new field on `Predicate` or `Entity`.
   - The phase modifies behaviour that a previous phase explicitly built (e.g. extends an existing dispatch).

   Then *recommend* the user write the design doc first, but do not refuse. Just note it in your output.
5. Draft a `/goal` template using this skeleton:
   ```
   /goal Implement Phase <N> of driver/tracks/<track_id>/plan.md (<phase name>):

   <one paragraph synthesising the unchecked bullets — these are what the goal should accomplish>

   <bullet list of the unchecked items verbatim>

   Prove by `cargo fmt --check` exit 0 (or the project's analogue), the project's test suite exits 0 with no regressions, and any phase-specific verification criteria from the bullets above. Tick the Phase <N> boxes in plan.md. Do not modify other phases in plan.md. Stop after <estimated_turns + 30% buffer, rounded to nearest 5> turns.
   ```

   If the project's CLAUDE.md or README references a non-Rust toolchain (e.g. `npm test`, `pytest`, `pnpm typecheck`), substitute the appropriate commands. If unsure, leave a `<fill in test command>` placeholder rather than guessing.

6. Print the drafted `/goal` text in a fenced code block. Above it, print: "Paste this into your CLI to fire the goal. Before doing so, optionally write `driver/tracks/<track_id>/phase<N>_design.md` and commit it." (Only mention the design doc if step 4 flagged it.)

**Do not invoke `/goal`. Do not modify any files. Do not commit. Just draft and print.**
