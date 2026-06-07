# 0006 — Shared core + performance consolidation

## Context
gandr grew tab-by-tab (Diff, then Repo) and feature-by-feature. A codebase review found
the two tabs evolved as **parallel implementations** with real duplication, plus per-keystroke
performance waste that bites at scale (the explicit goal is to be the fastest dev-tool /
file-manager of its kind — vscode-scale repos: ~15.6k files, ~1.3k-file diffs). Findings:

- **Performance (hot path = per keystroke, the loop only redraws on input/results):**
  - `App::tree_rows()` rebuilds the whole diff tree from scratch 2–3× per keystroke (no cache).
  - `Browser::rows()` clones the entire `Vec<BrowserRow>` on every call, and nav calls it
    repeatedly (sometimes only for `.len()`), so one arrow key clones a 15k-row vec several times.
  - The Repo-tab change-status map is rebuilt every frame instead of once per refresh.
  - The side-by-side viewer builds *every* row of the file every frame; the unified viewer
    already windows to O(viewport).
- **Duplication:** the "follow the cursor" scroll math is copy-pasted 4× (+ `anchor_near_top`
  2×); cursor movement, the async-highlight pipeline (2 spawners / events / caches), and several
  render primitives (`num_cell`, `base_bg`, pad-to-width, centered popup, selectable row) are
  duplicated.
- **Structure:** `app/mod.rs` is a 1878-line god-object (state + dispatch + 4 overlays + loop).
- **Sound already:** `compose::line_spans`, scrollbar, wrap, the async epoch/staleness model,
  the `git2`-behind-trait layering (ADR 0001), and "no panics in runtime" all hold.

## Decision
Consolidate the shared mechanics into a small core, **performance first**, without changing
*what* gandr is (DESIGN.md is unchanged — this is internal structure, not scope):

1. **Cache tree rows.** Both trees expose cached rows (invalidated by a version bump on the
   inputs) and hand out cheap `Rc` handles / borrows instead of rebuilding-and-cloning per
   keystroke. Navigation reads length/one-row without materializing the whole vec.
2. **Build derived state once per refresh,** not per frame (the Repo change-status map, search
   match lists).
3. **Window every viewer** to O(viewport) — the split viewer matches the unified one.
4. **`Viewport` type** (`cursor` + `scroll` + follow + `anchor_near_top`) replaces the 4 hand-
   rolled scroll-follow copies. A **`Selection`** (anchor, head) layers on top — built once,
   used by both viewers (enables copy-to-clipboard, a requested feature).
5. **Shared render primitives** in `ui` (`num_cell`/`base_bg`/`pad_to_width`/`centered_popup`/
   `selectable_row`); a `DiffRenderCtx` bundles the threaded render args (removes the
   `too_many_arguments` allows).
6. **Generic `HighlightCache<K>`** unifies the diff and preview highlight pipelines.
7. **Decompose `app/mod.rs`** over time: overlays into their own module behind an
   `Action`-returning `handle_key`; the run loop / editor launcher beside `jobs.rs`.

Tree *builders* stay separate (genuinely different data sources: a compacted changed-file set
vs a lazy filesystem walk) — only the row shape and renderer are shared.

## Consequences
- Navigation stays instant at vscode scale (the per-keystroke rebuild/clone is the current cap).
- One place to fix scroll/selection/highlight behavior; new features (clipboard selection,
  non-git file-manager mode) build on the shared core instead of forking a third copy.
- Pulling cursor/scroll/caches out of `&self`-render removes the `Cell`/`RefCell` double-borrow
  footgun (a latent panic risk, against the "no panics" rule).
- Done incrementally, one coherent commit per step, each behind the green gate + snapshot tests
  so behavior is preserved (snapshots change only intentionally).
