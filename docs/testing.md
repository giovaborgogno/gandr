# Testing & headless TUI verification

The hard part of a TUI: **you can't see it**. Everything here exists so an agent can verify
the UI without a human looking at a terminal. If you change rendering and don't update/inspect
a snapshot, you have not verified your change.

## Layers

### 1. Unit tests (pure logic)
Diff engine, tree building, base detection, config merge, search matching. Plain
`#[test]` table tests. No git, no terminal.

### 2. Fixtures (`testutil`)
Deterministic temp git repos built with `git2` + `tempfile`. Never test against gdiff's own
working tree (it changes). A fixture helper looks like:

```rust
let fx = testutil::Fixture::new();
fx.write("src/a.rs", "fn main() {}\n");
fx.commit("init");
fx.write("src/a.rs", "fn main() { let x = 1; }\n");   // uncommitted change
let backend = Git2Backend::open(fx.path())?;
let files = backend.changed_files(&CompareSpec::Uncommitted)?;
```

Keep fixture contents small and obvious so snapshots stay readable.

### 3. Snapshot / "e2e" tests (the UI)
Drive the real `App` with synthetic key events, render to ratatui's `TestBackend`, and
snapshot the buffer with `insta`. This is our deterministic end-to-end — no pty needed.

```rust
let mut app = App::new(backend, Config::test_defaults())?;
app.handle_key(key('j'));            // synthetic events
app.handle_key(key(']'));
let backend = TestBackend::new(100, 30);
let mut term = Terminal::new(backend)?;
term.draw(|f| app.render(f))?;
insta::assert_snapshot!(term.backend());   // golden text of the frame
```

For colors, snapshot a styled representation (cell fg/bg), not just glyphs, so palette
regressions are caught.

### Snapshot workflow
```bash
cargo test                 # fails if a snapshot changed
cargo insta review         # inspect each change; accept intended ones
```
- New/changed snapshots appear as `*.snap.new` (gitignored). **Read the diff** — never blanket
  `--accept`. Intended changes get committed as updated `.snap` files.

## `cargo run --example render`
A non-interactive binary that builds an `App`, renders one frame, and prints it as text to
stdout. Use it for a quick eyeball when iterating; it does not need a terminal. As of M0 it
renders an empty `App`; add fixture-backed scenarios (`--example render -- <scenario>`) as
the UI grows.

## What good coverage looks like per milestone
- M1: engine produces correct hunks/segments for add / delete / modify / rename / binary.
- M2+: a snapshot per view mode × a couple of representative fixtures (small modify, new
  file, deletion, multi-hunk). Navigation tests assert selection/scroll state.

## Not in v1
Real-PTY tests (`portable-pty`/`expectrl`) — heavier and flakier than TestBackend-driven
tests, and rarely worth it. Listed in `PLAN.md` backlog if we ever need true terminal e2e.
