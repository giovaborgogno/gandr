---
name: verify-tui
description: Render the gdiff TUI to plain text so you can inspect it without a real terminal. Use whenever you need to "see" the UI, check layout/rendering, or verify a UI change — instead of (uselessly) trying to run the interactive TUI.
---

# verify-tui

The TUI is invisible to an agent. This skill is how you look at it: render to text.

## Option A — snapshot test (preferred; deterministic, becomes a regression guard)

Add/extend a test that drives `App` and renders to ratatui's `TestBackend`, asserting with
`insta`. See `docs/testing.md` for the pattern. Then:

```bash
cargo test                 # see failures / new snapshots
cargo insta review         # inspect the rendered frame; accept intended output
```

Reading the snapshot file (committed `.snap` or pending `.snap.new`) shows you the exact
frame as text, including styles if the test captures them.

## Option B — example renderer (quick eyeball while iterating)

```bash
cargo run --example render                 # default scenario → frame printed to stdout
cargo run --example render -- <scenario>   # a named scenario, as scenarios are added
```

`examples/render.rs` builds a fixture repo + `App`, renders one frame, and prints it as text.
Extend it with new scenarios as the UI grows.

## Rules
- Never report a UI change as working without having rendered it via A or B.
- For color/style regressions, snapshot a representation that includes cell fg/bg, not just
  glyphs.
- Build fixtures with `testutil`; never render against gdiff's own working tree (it changes,
  so output isn't deterministic).
