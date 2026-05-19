Show the state of all Driver tracks in the current project. Read-only — never edit files, never commit.

## Steps

1. If `driver/tracks.md` does not exist, print: "No Driver tracks in this project. Run `/driver:new` to start one." and stop.
2. Read `driver/tracks.md`. Parse the bullet list — each entry is either `- [ ]` (open) or `- [x]` (done) followed by `[<track_id>](<path>) — <summary>`.
3. For each track:
   - **If `[x]`**: print one line `done — <track_id> — <summary>`.
   - **If `[ ]`**: read its `plan.md`. Count phases (lines matching `^## Phase \d+:`). Count phases where *every* bullet is `[x]`. Find the first phase with at least one `[ ]` bullet and report its name + remaining unchecked count. If all phases are complete, note that the track is ready for `/driver:close`.
4. Print as a tight table — one line per track, no JSON, no extra prose. Example output:
   ```
   Driver status (current project)

   open  20260518-grammar-expansion       phases 2/6 — next: Phase 3 (~40 turns), 12 unchecked
   open  20260601-perf-tuning             phases 0/3 — next: Phase 1 (~15 turns), 5 unchecked
   done  20260402-coverage-batch-two
   ```
5. If any open track has all phases complete but the registry line is still `[ ]`, append a single hint line at the end: "Track <id> looks done — run `/driver:close <id>`."

**Do not modify any files.** This is a status report only.
