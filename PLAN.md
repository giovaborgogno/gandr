# gdiff — Implementation Plan

> The live roadmap. Agents pick the **next unchecked milestone**, implement it, make it
> green (see "Definition of done"), tick the boxes, and commit. One commit per milestone,
> directly to `main` (see AGENTS.md → Workflow).
>
> `DESIGN.md` = what we're building (locked). This file = how/when, and current status.

## Status

- [x] M0 — Scaffold + skeleton
- [x] M1 — Git + diff model (no TUI)
- [x] M2 — Core TUI (unified)
- [x] M3 — Delta-style rendering
- [x] M4 — Tree + side-by-side
- [x] M5 — Compare picker + smart + PR
- [x] M6 — Live watch + review state
- [x] M7 — DX polish (search, editor, theme auto, help) — partial; see notes

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

### M1 — Git + diff model (no TUI) ✅
- [x] `GitBackend` trait + `git2_backend` for `CompareSpec::Uncommitted`.
- [x] `diff::engine` via imara-diff → `FileDiff` (line hunks + context folding). Word segments stubbed (M3).
- [x] `testutil` fixture helper: build temp git repos with known changes (git2 + tempfile).
- [x] Unit tests on the diff engine against fixtures (modify, add, delete, rename, binary, multi-hunk, context cap).
- [x] Temporary debug printer (`cargo run --example dump_diff`).
- Deliverable: `cargo test` proves the engine produces correct diffs.

### M2 — Core TUI (unified) ✅
- [x] Layout: header / file list (flat) / unified viewer / keybar.
- [x] Hybrid navigation (Tab focus; n/p file; ]/[ hunk; j/k/g/G/Ctrl-d/u).
- [x] Scroll + selection + sticky file header (with effective-scroll clamp).
- [x] Snapshot tests for unified rendering across fixtures (modify, multi-file, nav, scroll, empty).
- Deliverable: a usable unified diff viewer driven entirely by keyboard.

### M3 — Delta-style rendering ✅
- [x] Background colors for add/del lines; line-number gutters; colored change bar.
- [x] Word-level segments (imara-diff at word granularity) → stronger bg; `w` toggle.
- [x] syntect highlighting (two-face, per-line) + `compose` (syntax fg over diff bg over word bg).
- [x] Style-aware snapshot tests (background-legend map) + word-toggle + syntax-fg + multibyte safety.
- Deliverable: looks like delta.

### M4 — Tree + side-by-side ✅
- [x] Compact folder tree (collapse single-child dirs), expand/collapse (←/→/Enter), markers.
- [x] Tree viewport follows the cursor (`tree_scroll`).
- [x] `s` side-by-side viewer with aligned line wrapping (rows expand to the taller cell).
- [x] Unit tests for tree building + snapshot tests for tree + split view.

### M5 — Compare picker + smart + PR ✅
- [x] `c` compare-picker overlay; all `CompareSpec` variants wired through git2_backend
      (Uncommitted/Staged/WorkdirVs/Range/Commit).
- [x] Base detection via merge-base (`detect_base`); `base::resolve` smart fallback chain.
- [x] `--pr [N]` via `gh pr view` (tsv, title-last so tabs are safe); header shows PR title.
- [x] CLI parsing (`<ref>`, `<ref>..<ref>`, `--staged`, `--pr`, `--smart`); Ctrl-C quits.
- [x] Tests: cli parsing, all backend comparison kinds, detect_base, picker overlay snapshot.
- Note: path scoping (`gdiff <path>`) deferred (ref/path disambiguation) — see backlog.

### M6 — Live watch + review state ✅
- [x] notify + debouncer auto-refresh (working-tree comparisons), preserving selection + scroll.
- [x] Review state persisted to `.git/gdiff/state.json` (serde_json), keyed by comparison.
- [x] Changed-since-reviewed `⚠` badge (content-hash); `✓` reviewed; `N/M reviewed` in header.
- [x] `Space` review, `a` toggles auto-refresh, `r` manual refresh; `◉ watching` indicator.
- [x] Review status cached (recomputed on refresh/toggle/spec-change, not per frame).
- Note: a transient "updated" flash was dropped in favor of the steady `◉ watching` indicator.

### M7 — DX polish (partial) ✅
- [x] In-diff search (`/`) with `n`/`N` match navigation (jumps the viewer).
- [x] Open in editor (`e`) — `$VISUAL`/`$EDITOR`, suspends/restores the TUI, `+line` / `code -g`.
- [x] Help overlay (`?`); empty state (from M2); overlays clamp to tiny terminals.
- [x] Theme `auto` detection (termbg/OSC 11) + light/dark palettes & syntect themes.
- [ ] **Deferred to backlog** (see below): config-file loading (TOML), copy (`y`),
      context expand (`o`), mouse, and tree-filter search.

---

## v2 — post-v1 roadmap (requested)

- [ ] M8 — Tabs + repo file browser. gitui-style tab bar (`Diff [1] · Files [2]`).
      New **Files** tab: lazy tree of the whole working tree (incl. git-ignored
      files/folders; only `.git/` is skipped), selecting a file shows its content
      syntax-highlighted.
- [x] M9 — Branch/ref picker: pick *and fuzzy-search* any branch/tag to compare.
      `b` (or the `c` compare menu) opens a fuzzy picker over all local/remote
      branches + tags (fzf-style `fuzzy::score`, smart-case); Enter compares the
      working tree against it (`WorkdirVs`). Backed by `GitBackend::list_refs`.
- [x] M10 — Search across all files (jump to file + match), not just the current file.
      *Delivered by M14 (the repo-wide content search jumps to file + line).*
- [x] M11 — Expand context (`o`): cycles the context window 3→10→30→100 (then
      wraps), recomputing the diff async to reveal more lines around every hunk
      (`git diff -U<n>`-style). Header shows `⊕N ctx` while expanded. (A per-gap
      "⋯ expand here ⋯" affordance remains a future refinement — see backlog.)
- [x] M12 — Multi-line syntax highlighting (carry syntect state across a file).
      The Files-tab content viewer highlights the whole file with one
      `HighlightLines` (block comments / multi-line strings render correctly),
      computed once at load (cached in `Loaded.highlights`), re-run on theme
      change. The diff viewer still highlights per-line — correct old/new
      multi-line there needs full file text threaded in (see backlog).
- [ ] M13 — **Async architecture** (decided: worker threads + crossbeam channels,
      NOT tokio — git2/FS/grep are blocking libs, so a thread pool + an
      epoch token for superseding stale work is the right model). Unified event
      loop (terminal input + job results + file-watch on one channel). Heavy
      work (diff recompute, file load, search) runs off the UI thread; results
      post back as events. Initial diff stays sync (fast startup + tests).
- [x] M14 — **Repo-wide search via embedded crates**: `ignore` for file-name
      search (fd-style, respects .gitignore) and `grep`/`grep-searcher` for
      content search (ripgrep-style) — no external binaries. Search runs async
      (M13). `/` is now: in-diff (Diff tab) + repo-wide (Files tab) with a
      results list that jumps to file + line; `Tab` flips file-name ↔ content.
      Walk is sorted (deterministic) and capped at `search::MAX_RESULTS` (500).

## Backlog / later (not v1)

- ~~**Branch/ref picker**~~ — done in M9 (`b` opens a fuzzy picker over branches +
  tags). A possible follow-up: include arbitrary commits / reflog entries, and let
  the chosen ref be either side of a range (currently it's always `WorkdirVs`).
- ~~**Search across all files**~~ — done in M14 (Files-tab `/` searches names + content
  repo-wide and jumps to file+line). A possible follow-up: debounce keystrokes so a
  broad query on a huge repo doesn't spawn a walk per character (the epoch already
  drops stale results, so it's a CPU optimization, not a correctness issue).
- **Multi-line highlighting in the *diff* viewer** — M12 did the Files-tab
  whole-file viewer. The diff viewer still highlights each line in isolation;
  doing it correctly there means highlighting the new file and the old file each
  in order (carrying state) and mapping spans to displayed lines by line number —
  needs the full old/new text available to the UI + caching (it isn't today).
- **Config-file loading** (`~/.config/gdiff/config.toml` + per-repo `.gdiff.toml`,
  `[colors]`/`[keys]`) — the `Config` struct + `theme = auto` resolution exist; only
  TOML parsing/merging is unbuilt (would add a `toml` dep). Defaults work today.
- **Copy** (`y`) path/selection to clipboard (needs `arboard` or OSC 52).
- ~~**Per-gap context expand**~~ — done. The diff viewer now folds the file's
  full line list for display: collapsed gaps render as `⋯ N unchanged lines ⋯`
  markers, and `Enter` (diff focused) expands the gap nearest the viewport top.
  `o` still sets the global base context (it now just re-folds — no diff rebuild).
  A possible refinement: incremental reveal (expand a few lines at a time, both
  directions) instead of revealing the whole gap.
- **Mouse** (scroll + click-to-select-file) — enable crossterm mouse capture + hit-testing.
- **Tree-filter search** — `/` currently searches the diff; filtering the file tree by
  query (contextual on tree focus) is unbuilt.
- Tab expansion (`config.tab_width`) and display-width-aware background fill
  (`unicode-width`) in the viewer — currently the bg fill uses char count, so
  CJK/wide chars and tabs can leave the delta background slightly short of the
  edge (cosmetic; no panic — ratatui truncates overflow).
- Real-PTY e2e smoke test (`portable-pty`/`expectrl`).
- Tree view for huge PRs: virtualized rendering.
- PR description / metadata side panel.
- Merge-conflict view.
- `gix` backend implementation (swap behind `GitBackend`).

## Pre-publish (before flipping `publish` / open-sourcing)

- [ ] Decide license (Cargo.toml currently declares `MIT OR Apache-2.0`); add `LICENSE-MIT` + `LICENSE-APACHE`.
- [ ] Consider renaming the repo directory `difftui/` → `gdiff/` to match the crate.
- [ ] Polish README (install, usage, screenshot/asciinema), add crates.io metadata, set `publish = true`.
