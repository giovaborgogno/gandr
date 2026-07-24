# Architecture Decision Records

Short records of *why* we chose what we chose, so future agents don't relitigate settled
calls (or relitigate them deliberately, with context). Format: context → decision →
consequences. Add a new numbered file to change a decision; don't edit history, supersede it.

- [0001](0001-git2-behind-trait.md) — git2 backend behind a `GitBackend` trait (gix later)
- [0002](0002-imara-diff-for-line-and-word.md) — imara-diff for both line- and word-level diffs
- [0003](0003-no-stdout-parsing.md) — no shelling-out-to-git stdout parsing for diffs
- [0004](0004-theme-auto-detect.md) — `theme = "auto"` via terminal background (OSC 11)
- [0005](0005-readonly-and-direct-commits.md) — read-only viewer; direct commits to main
- [0006](0006-shared-core-and-perf-consolidation.md) — shared core + performance consolidation
- [0007](0007-search-scopes-slash-and-finder.md) — search scopes: `/` in-view, `f`/`F` finder
- [0008](0008-image-preview.md) — image preview via `ratatui-image` (protocol + halfblocks)
