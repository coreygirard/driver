Close a Driver track. Verifies that every phase in the track's `plan.md` is fully checked, then flips the track's registry line in `driver/tracks.md` to `[x]` and commits.

## Inputs

Required positional argument: `<track_id>`. Refuse if missing.

## Steps

1. Read `driver/tracks/<track_id>/plan.md`. If missing, refuse: "No such track."
2. Scan the file for `- [ ]` bullets. If **any** unchecked bullet exists, refuse:
   ```
   Track <track_id> still has unchecked bullets:
     Phase <N> (<name>): <count> unchecked
     Phase <M> (<name>): <count> unchecked
   Finish them via `/driver:next` + `/goal` before closing.
   ```
   Stop without modifying anything.
3. Read `driver/tracks.md`. Find the line matching the bullet for `<track_id>`. Verify it starts with `- [ ]` — if it's already `[x]`, print "Already closed." and stop.
4. Rewrite that line, changing `- [ ]` to `- [x]`. Preserve everything else verbatim (link, summary).
5. `git add driver/tracks.md` and commit with message `driver: close track <track_id>`. Use the project's standard Co-Authored-By footer if one is in CLAUDE.md.
6. Print one line: `Closed <track_id>.`

**Do not modify any plan.md or phase design docs. This command only flips one line in tracks.md and commits.**
