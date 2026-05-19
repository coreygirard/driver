Show the state of all Driver tracks in the current project. Shells out to the `driver` CLI; this skill is just a wrapper.

## Steps

1. Run `driver status`. Print its output verbatim.
2. If the CLI exits non-zero or prints "no driver/tracks.md found", suggest running `/driver:new`.

**Do not parse plan.md or tracks.md by hand.** The CLI is the source of truth for parsing.
