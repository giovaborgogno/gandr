# 0003 — No shelling-out-to-git stdout parsing for diffs

## Context
delta works by parsing `git diff` unified output from stdin (a state machine over
`@@` hunks, `+/-/ ` lines, rename headers, etc.). It's a viable approach but the parsing is
the fragile, fiddly part.

## Decision
Do **not** obtain diffs by parsing `git`'s textual output. Get structured data from the
backend (ADR 0001) — changed files + raw file contents — and compute the diff ourselves
(ADR 0002). We keep full structured control end-to-end.

## Consequences
- No unified-diff state machine to maintain; fewer edge cases (rename headers, mode lines,
  `\ No newline at end of file`, combined diffs).
- We control hunking and context folding directly, which the UI needs anyway.
- We *do* still shell out to `gh` for PR metadata and may shell out to `git` for narrow,
  well-defined queries (e.g. merge-base) where it's simplest — that's not diff parsing.
- Background-color rendering (the delta look the user wants) is purely a ratatui styling
  concern, independent of how diffs are obtained.
