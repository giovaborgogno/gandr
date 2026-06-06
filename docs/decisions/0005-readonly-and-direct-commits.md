# 0005 — Read-only viewer; direct commits to main

## Context
Two scope/process calls set by the project owner.

## Decision
1. **gdiff is a read-only viewer.** It never mutates the target repo's working tree, index,
   or refs. No staging, committing, conflict resolution, or comment posting. The sole write
   is gdiff's own review state under `.git/gdiff/`.
2. **Development of gdiff commits directly to `main`**, one commit per milestone (or coherent
   sub-task). No PRs for gdiff's own development. Quality is enforced by the
   "Definition of done" gate (fmt + clippy + test) and CI, not by review gates.

## Consequences
- Simpler mental model and codebase: no mutation paths to design, test, or guard.
- Faster iteration for agents; no PR ceremony.
- The safety net is the automated gate + CI, so it must stay green — a broken `main` blocks
  everyone. Run the gate before every commit.
- If gdiff is later open-sourced (see PLAN "pre-publish"), external contributions would move
  to PRs; this ADR would be superseded for that context.
