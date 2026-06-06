---
name: next-milestone
description: Pick the next unchecked milestone in PLAN.md, implement it end-to-end, make it pass the Definition-of-done gate, tick the boxes, and commit to main. Use when asked to "continue gandr", "do the next milestone", "keep building", or work the roadmap.
---

# next-milestone

Drives gandr development one milestone at a time, autonomously, following the repo's rules.

## Steps

1. **Orient.** Read `PLAN.md` (status + the next unchecked milestone), `DESIGN.md` (the
   locked spec for what to build), `AGENTS.md` (golden rules), and `docs/architecture.md`.
   Identify the next unchecked milestone `Mx` and its sub-tasks.

2. **Confirm scope.** Implement exactly that milestone — no skipping ahead, no scope creep.
   If the milestone seems to require a `DESIGN.md` change, stop and surface it (a design
   change needs an ADR in `docs/decisions/` + owner sign-off), don't silently redesign.

3. **Implement.** Respect the layering (`ui` → `diff`/`highlight` → `git` trait; never call
   `git2::*` outside `src/git/`). No `unwrap`/`panic` in runtime code. English everywhere.

4. **Verify headlessly — this is mandatory for any UI work.** You cannot see the TUI. Add or
   update snapshot tests (`TestBackend` + `insta`, see `docs/testing.md`) and/or use
   `cargo run --example render`. Do not claim UI behavior works from reading code.

5. **Run the Definition-of-done gate:**
   ```bash
   cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
   ```
   Fix until green. Review any new `*.snap.new` with `cargo insta review` (read the diff;
   never blanket-accept).

6. **Update PLAN.md.** Tick the milestone's sub-task boxes and the top-level milestone box.

7. **Run `/code-review`** on the pending diff and address its findings (fix real issues, or
   consciously decline with a reason). Mandatory before every commit — no exceptions for
   "small" or "docs-only" changes.

8. **Commit to main** (one commit per milestone; conventional message). Include the PLAN.md
   update in the same commit. End the message with the Co-Authored-By trailer.

9. **Report** what you built, what tests cover it, and what the next milestone is. Stop there
   unless told to continue.

## Guardrails
- Read-only product: never add code that mutates the target repo (ADR 0005).
- Verify imara-diff 0.2 / ratatui 0.30 / crossterm 0.29 APIs against installed docs, not memory.
- If the gate can't go green for reasons outside the milestone, surface it rather than
  hacking around it.
