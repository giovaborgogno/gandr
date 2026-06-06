# gdiff — Implementation Plan

> The live roadmap. Agents pick the **next unchecked milestone**, implement it, make it
> green (see "Definition of done"), tick the boxes, and commit. One commit per milestone,
> directly to `main` (see AGENTS.md → Workflow).
>
> `DESIGN.md` = what we're building (locked). This file = how/when, and current status.

## Status

- [x] M0 — Scaffold + skeleton
- [ ] M1 — Git + diff model (no TUI)
- [ ] M2 — Core TUI (unified)
- [ ] M3 — Delta-style rendering
- [ ] M4 — Tree + side-by-side
- [ ] M5 — Compare picker + smart + PR
- [ ] M6 — Live watch + review state
- [ ] M7 — DX polish (search, context expand, editor, theme, config, mouse)

Legend for sub-tasks: `[ ]` todo, `[x]` done.

## Architecture (target)

```
src/
  main.rs            # parse args → Config → App::run
  cli.rs             # args → CompareSpec + flags
  config.rs          # load/merge config + theme resolution, persistence paths

  git/
    mod.rs           # GitBackend trait + CompareSpec + DTOs (FileChange, FileContents)
    git2_backend.rs  # git2 impl: resolve refs, list changed files, fetch old/new contents
    base.rs          # base-branch detection (merge-base); PR resolution via `gh`

  diff/
    mod.rs           # model: FileDiff, Hunk, Line, LineKind, Segment
    engine.rs        # imara-diff: line hunks + intra-line word segments
    context.rs       # context folding / expandable regions

  highlight/
    mod.rs           # syntect: SyntaxSet/ThemeSet (two-face), per-file cache
    compose.rs       # merge syntax fg + diff bg + word-level bg → ratatui Spans

  ui/
    mod.rs           # frame layout (header / tree / viewer / keybar)
    tree.rs          # compact file tree widget
    viewer_unified.rs
    viewer_split.rs  # side-by-side with wrap + alignment
    gutter.rs        # line-number columns
    compare_picker.rs / help.rs / search.rs   # overlays

  app/
    mod.rs           # App state, focus, key dispatch, event loop
    state.rs         # ReviewState (persisted), view mode, scroll, selection
    events.rs        # crossterm input + watcher + async-diff channel merge
    async_diff.rs    # background diff compute (thread + crossbeam channel)
    watcher.rs       # notify + debouncer → refresh events
```

## Core data model (target)

```rust
enum CompareSpec { Uncommitted, Staged, WorkdirVs(Rev), Range(Rev, Rev), Commit(Rev), Pr(Option<u32>) }

struct FileChange { path, old_path: Option<PathBuf>, status: Status, is_binary, additions, deletions }
struct FileDiff   { change: FileChange, hunks: Vec<Hunk> }
struct Hunk       { old_start, new_start, header: String, lines: Vec<Line> }
struct Line       { kind: Context|Add|Del, old_no: Option<u32>, new_no: Option<u32>, text: String, segments: Vec<Segment> }
struct Segment    { start: usize, end: usize, changed: bool }   // byte range into text
```

---

## Milestones

Each milestone is independently runnable and ends in a commit. After each, run the
**Definition of done** gate.

### M0 — Scaffold + skeleton ✅
- [x] Module tree from "Architecture" exists (empty/stub modules compile).
- [x] `App::run` opens an alt-screen ratatui frame and quits on `q`.
- [x] First snapshot test infra wired (`TestBackend` + `insta`) — one trivial passing snapshot.
- [x] `examples/render.rs` stub: renders a frame to stdout (the agent's "eyes").
- [x] CI green (gate: fmt + clippy -D warnings + test).
- Deliverable: runs, shows an empty frame, quits cleanly.

### M1 — Git + diff model (no TUI)
- [ ] `GitBackend` trait + `git2_backend` for `CompareSpec::Uncommitted`.
- [ ] `diff::engine` via imara-diff → `FileDiff` (line hunks). Word segments may be stubbed.
- [ ] `testutil` fixture helper: build temp git repos with known changes (git2 + tempfile).
- [ ] Unit tests on the diff engine against fixtures.
- [ ] Temporary debug printer (stdout) to eyeball diffs.
- Deliverable: `cargo test` proves the engine produces correct diffs.

### M2 — Core TUI (unified)
- [ ] Layout: header / file list (flat first) / unified viewer / keybar.
- [ ] Hybrid navigation (Tab focus; n/p file; ]/[ hunk; j/k/g/G/Ctrl-d/u).
- [ ] Scroll + selection + sticky file header.
- [ ] Snapshot tests for unified rendering across fixtures.
- Deliverable: a usable unified diff viewer driven entirely by keyboard.

### M3 — Delta-style rendering
- [ ] Background colors for add/del lines; line-number gutters; hunk bar.
- [ ] Word-level segments (imara-diff at word granularity) → stronger bg; `w` toggle.
- [ ] syntect highlighting + `compose` (syntax fg over diff bg over word bg).
- [ ] Snapshot tests covering colored output (insta captures styles).
- Deliverable: looks like delta.

### M4 — Tree + side-by-side
- [ ] Compact folder tree (collapse single-child dirs), expand/collapse, markers.
- [ ] `s` side-by-side viewer with line wrapping + alignment.
- [ ] Snapshot tests for tree + split view.

### M5 — Compare picker + smart + PR
- [ ] `c` compare-picker overlay; all `CompareSpec` variants wired through the backend.
- [ ] Base-branch detection via merge-base; `--smart` fallback chain.
- [ ] `--pr [N]` via `gh pr view --json`; header shows PR title/number.

### M6 — Live watch + review state
- [ ] notify + debouncer auto-refresh (working-tree comparisons), preserving scroll/selection.
- [ ] Review state persisted to `.git/gdiff/state.json`, keyed by comparison.
- [ ] Changed-since-reviewed `⚠` badge logic.
- [ ] "updated" flash; `a` toggles auto-refresh; `r` manual refresh.

### M7 — DX polish
- [ ] Contextual search (`/`) + match nav; context expand (`o` / "expand context").
- [ ] Open in editor (`e`); copy (`y`); help overlay (`?`); mouse (scroll + click).
- [ ] Config loading/merging (`~/.config/gdiff` + `.gdiff.toml`) + `[colors]`/`[keys]`.
- [ ] Theme `auto` detection (termbg/OSC 11) + light/dark palettes.
- [ ] Empty state.

---

## Backlog / later (not v1)

- Real-PTY e2e smoke test (`portable-pty`/`expectrl`).
- Tree view for huge PRs: virtualized rendering.
- PR description / metadata side panel.
- Merge-conflict view.
- `gix` backend implementation (swap behind `GitBackend`).

## Pre-publish (before flipping `publish` / open-sourcing)

- [ ] Decide license (Cargo.toml currently declares `MIT OR Apache-2.0`); add `LICENSE-MIT` + `LICENSE-APACHE`.
- [ ] Consider renaming the repo directory `difftui/` → `gdiff/` to match the crate.
- [ ] Polish README (install, usage, screenshot/asciinema), add crates.io metadata, set `publish = true`.
