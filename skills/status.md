Show the state of the Driver project. Shells out to the `driver` CLI; this skill is just a wrapper.

## Steps

1. Run `driver status`. Print its output verbatim.
2. If the CLI exits non-zero or prints "no driver/ project found", suggest running `/driver:new`.

**Do not parse plan.md by hand.** The CLI is the source of truth — task state is parsed from `driver/plan.md` at read time.
