Edit `driver/spec.md` — the project's contract layer. Bullets describe what the project commits to being; each bullet gets a content-derived 6-char hash, so editing a bullet automatically gives it a new identity. Spec is the source of truth that `/driver:go` reads to synthesize implementation tasks.

**This skill never touches `plan.md` or implementation.** Spec edits are user-driven; implementation is agent-driven via `/driver:go`. Keep the two phases separate — that's what prevents "agent rewrote the spec to match what it built."

## Inputs — ask the user via `AskUserQuestion`

This skill captures the user's spec intent. **Always ask via `AskUserQuestion` before editing**, even when the user has set an autonomous / no-clarifying-questions flag for the session. Choosing *what the project commits to being* is the exact decision this skill exists to capture.

Ask in one prompt:

- **What to change**: add bullets, refine an existing bullet, or remove one.
- **The substance**: one paragraph describing the addition/change. The agent translates this into one or more spec bullets.

If `spec.md` doesn't exist yet, the first question is "what does this project commit to being?" — capture 3–8 high-level bullets describing capabilities, invariants, or external behaviors. Don't seed a long list; the user can run `/driver:spec` repeatedly.

## Spec bullet format

```markdown
# <Project name>

<1–2 sentence preamble: what this project is.>

- **<optional-short-name>** Bullet body — what the project commits to.
  Continuation lines indented two spaces.
- Bullet body without a name. Still gets a content hash.
```

Rules:

- **One commitment per bullet.** If you're tempted to use "and", split.
- **Falsifiable.** The bullet should describe something *visible* in code or tests. "Users can authenticate with email" is good; "the system is well-designed" is not.
- **Names are optional and not hashed.** They're human labels. Editing the name doesn't change the hash; editing the body does.
- **Don't include implementation choices** unless the choice is the commitment ("uses SQLite" only if cross-DB portability is *not* a commitment).

## Steps

1. Confirm a git repo (`git rev-parse --git-dir`). If not, refuse: "Driver expects a git repo. Run `git init` first."

2. If `driver/spec.md` exists, run `driver spec list` to show current bullets with their hashes and status. Read it before editing — you may be repeating an existing commitment.

3. Edit `driver/spec.md` to reflect the user's intent:
   - **Adding**: append new bullets at the end of the relevant section (or under a new `## Section` if the file uses sections).
   - **Refining**: edit the bullet text in place. The hash will change automatically; that's the point.
   - **Removing**: delete the bullet line(s). Any tasks that referenced its hash become orphan references, surfaced by `driver spec list`.

4. After editing, run `driver spec list` again. If there are orphan references (hashes referenced in `plan.md` that no longer exist in `spec.md`), surface them to the user:
   - If the edit was cosmetic (typo, rewording, no change in commitment): suggest `driver spec amend <old> <new>` to migrate task references.
   - If the edit was semantic (the commitment really changed): leave the orphan — the old work satisfied the old commitment; the new bullet will be picked up as unmet by `/driver:go`.

5. `git add driver/spec.md` and commit with `driver: spec — <one-line summary of the change>`.

6. Print:
   ```
   spec.md updated.
   - N bullets total (M met, P planned, U unmet)
   - <optional: orphan-reference warning>
   Next: run /driver:go "<high-level guidance>" to synthesize tasks for the unmet bullets.
   ```

**Stop after the commit.** Do not run `/driver:go`. Do not propose plan tasks. Do not implement. The user decides when to move from spec to implementation.

## What goes in spec.md vs principles.md vs plan.md

- **spec.md**: *what* the project commits to being (capabilities, invariants). Read by `/driver:go` to drive synthesis.
- **principles.md**: *which file changes need explicit consent*. Read by `driver tick` to enforce the mechanical floor.
- **plan.md**: *the work in flight* (open tasks + ticked history). Tasks link back to spec bullets via `[spec: <hash>]`.

A bullet that doesn't fit any of these doesn't belong in Driver — write a CLAUDE.md or README note instead.
