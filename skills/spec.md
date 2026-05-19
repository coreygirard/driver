Interrogate the user about what the project commits to being, and write the result to `driver/spec.md`. **This skill is the project's contract layer** — it is where the high-signal interaction lives. The goal is that after a real `/driver:spec` conversation, the agent can derive any task from the spec without asking which to do next.

**This skill never touches `plan.md` or implementation.** Spec edits are user-driven; implementation is agent-driven via `/driver:go`. That separation is what prevents "agent rewrote the spec to match what it built."

## Stance: interrogate, don't transcribe

The bad version of this skill takes whatever the user types and writes it down as bullets. The good version pushes back. The pressure of writing a contract is the feature, not the bug — front-loading the high-signal conversation is what lets `/driver:go` run lights-out afterwards.

If a user says "I want to build a web renderer," the bad skill writes `- The system is a web renderer.` and stops. The good skill asks:

- *What's the falsifiable target?* ("Chromium parity on these 40 CSS properties" beats "renders correctly")
- *What's the shape of the work?* ("a long pile of narrow CSS-feature slices verified against an oracle" vs "a sequence of distinct phases")
- *What cross-cutting decisions span every task?* (oracle architecture, parity definition, test infrastructure, perf budget)
- *What's explicitly out of scope?* (subpixel font rendering, audio, animation)

Then writes ~5–15 bullets that an agent can actually derive work from. **The goal is bullets that are falsifiable from the repo** — something a test or a `grep` could check.

## The 4-category walkthrough

For a new spec (or a spec missing one of these categories), walk all four. Use `AskUserQuestion` for each — one prompt per category, with `multiSelect: false` and a focused question. Push back if the answer is vague or unfalsifiable.

### 1. Target — what does "done" look like?

Ask: *"What's the falsifiable target this project commits to? Phrase it so that 'are we done?' has a check, not a vibe."*

Good targets (capture as `## Target` section bullets):
- `Renders the 40 CSS properties listed below to per-pixel parity with Chromium's getBoundingClientRect on every oracle test.`
- `Round-trips every valid TOML 1.0 document: parse → serialize → parse yields the same AST.`
- `Solves the daily Wordle in ≤5 guesses on 95%+ of the past two years of puzzles.`

Bad targets (push back):
- `Builds a great web renderer.` → *How do I check "great"?*
- `Has good test coverage.` → *What property does the test coverage protect?*
- `Production-ready.` → *Production for whom doing what?*

### 2. Shape — what's the structure of the work?

Ask: *"Is the work one big pile of similar slices (each a small parity slice or feature add), a sequence of distinct epics (parsing → typechecking → codegen), or one tight loop (build the eval harness, then iterate inside it)?"*

Capture as `## Shape` section bullets. The shape tells the agent how to think about ordering and dependencies. A "pile of slices" means slice order doesn't carry signal — the agent should never escalate "which next?" A "sequence of epics" means cross-epic dependencies matter and need to be in `[depends: ...]`. A "tight loop" means there's one infrastructure task first, then iterations.

### 3. Architecture — what cross-cutting decisions span every task?

Ask: *"What architectural commitments span every task in this project? Things like: how is correctness verified, what's the test/oracle setup, what languages/runtimes are pinned, what invariants hold across the codebase, what performance budget."*

This is the highest-leverage category. **The kind of decisions that should normally fire `driver ask --halt` during implementation belong here instead, captured upfront.** If the user can name the oracle architecture in advance, you don't have to escalate it from inside the `z-index` task three hours from now.

Examples (capture as `## Architecture` section bullets):
- `Correctness is verified by a Chromium oracle running headlessly; every layout feature ships with at least one oracle test.`
- `Paint-order tests use elementsFromPoint queries since getBoundingClientRect doesn't observe stacking.`
- `All length values stored as f64 post-cascade; cascade resolution happens once, not at use site.`
- `No new dependencies without an explicit ask; the test runner is the project's existing cargo test.`

### 4. Out of scope — what is the project explicitly NOT?

Ask: *"What's tempting to include but explicitly out of scope? Listing non-goals prevents drift."*

Capture as `## Out of scope` section bullets. These are negative commitments and are valuable precisely because they shut down future "should we add X?" questions.

## Inputs

You **MUST** ask via `AskUserQuestion`, even when the user has set an autonomous flag for the session. Choosing what the project commits to being is the exact decision this skill exists to capture; the autonomous flag does not apply.

Ask one category at a time. After each answer, if the substance is too thin to act on, push back with one follow-up before moving on. "Push back" means a second `AskUserQuestion` with sharper options, not a paragraph of skepticism.

If the user explicitly says "skip" or "I don't know yet" for a category, respect it — but write a single `## <Category> — TBD` line as a placeholder. `/driver:go` will be more likely to escalate questions in the under-specced area, and that's the correct behavior.

## Spec bullet format

```markdown
# <Project name>

<1–2 sentence preamble: what this project is at the highest level.>

## Target

- **<optional-name>** A capability or invariant the project commits to.
  Continuation indented two spaces.

## Shape

- Bullet describing the structure of the work.

## Architecture

- **<optional-name>** Cross-cutting decision.

## Out of scope

- Explicit non-goal.
```

Rules:

- **One commitment per bullet.** If you're tempted to use "and", split.
- **Falsifiable.** The bullet should describe something *visible* in code, tests, or the build. "Users can authenticate with email" → a `/login` endpoint either accepts email or doesn't. "The system is well-designed" → push back, not a bullet.
- **Names are optional and not hashed.** They're human labels. Editing the name doesn't change the hash; editing the body does. Use names for bullets you expect to be referenced often (the high-leverage architecture commitments).
- **Section headers are visual only.** The CLI parses bullets regardless of which section they live in. The categories shape the conversation, not the parser.

## Steps

1. Confirm a git repo (`git rev-parse --git-dir`). If not, refuse: "Driver expects a git repo. Run `git init` first."

2. If `driver/spec.md` exists, run `driver spec list` to show current state. Identify which of the 4 categories are present (look at section headers). Only walk the missing or thin categories.

3. For each missing or thin category, run the 4-category walkthrough above. Push back on vague answers with one focused follow-up.

4. Edit `driver/spec.md`:
   - **First-time scaffold**: write the full skeleton with all four `## Section` headers, even if some are TBD.
   - **Extending**: append bullets to the relevant section. Don't reorder existing bullets — that would generate spurious hash changes.
   - **Refining**: edit the bullet text in place. The hash will change automatically; that's the point.

5. Run `driver spec list`. If there are orphan references (hashes referenced in `plan.md` that no longer exist in `spec.md`), surface them:
   - If the edit was cosmetic: suggest `driver spec amend <old> <new>`.
   - If the edit was semantic: leave the orphan — old work satisfied the old bullet; the new bullet will be picked up as unmet by `/driver:go`.

6. `git add driver/spec.md` and commit with `driver: spec — <one-line summary>`.

7. Print:
   ```
   spec.md updated.
   - N bullets total (M met, P planned, U unmet)
   - Categories: Target (n), Shape (n), Architecture (n), Out of scope (n)
   - <optional: orphan warnings>
   Next: /driver:go to synthesize tasks for the unmet bullets.
   ```

**Stop after the commit.** Do not run `/driver:go`. Do not propose plan tasks. Do not implement.

## What goes in spec.md vs principles.md vs plan.md

- **spec.md**: *what* the project commits to being (capabilities, invariants, architecture). Read by `/driver:go` to drive synthesis.
- **principles.md**: *which file changes need explicit consent* (file-glob rules). Read by `driver tick` to enforce the mechanical floor.
- **plan.md**: *the work in flight* (open tasks + ticked history). Tasks link back to spec bullets via `[spec: <hash>]`. Derived from spec, not authored directly.

A bullet that doesn't fit any of these doesn't belong in Driver — write a CLAUDE.md or README note instead.
