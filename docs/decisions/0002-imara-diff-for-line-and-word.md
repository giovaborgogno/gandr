# 0002 — imara-diff for both line- and word-level diffs

## Context
The product's value is delta-style rendering: not just which lines changed, but which
**tokens within a line** changed. No git backend gives word-level diffs for free (neither
git2 nor gix). delta hand-rolls a ~450-line Needleman-Wunsch aligner.

## Decision
Use **`imara-diff`** — the fast Myers/histogram engine that gitoxide and Helix use — for
*both* granularities. It is generic over a token interner:
- Pass 1: intern **lines** → line-level hunks.
- Pass 2: intern **words** within an aligned removed/added line pair → changed spans (`Segment`s).

## Consequences
- One fast, well-tested engine instead of a custom aligner.
- Word-level becomes "feed it word tokens", not bespoke code.
- imara-diff **0.2** reworked the API vs 0.1 — verify against installed docs, not memory.
- Line/word alignment heuristics (when to even attempt word-diff a pair) live in
  `src/diff/engine.rs` and are ours to tune.
