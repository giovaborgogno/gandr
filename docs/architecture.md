# Architecture

Layered, one-directional dependencies. Upper layers depend on lower; nothing depends back up.

```
            ┌──────────────┐
            │     app/     │  state, event loop, focus, key dispatch, watcher, async
            └──────┬───────┘
                   │ uses
        ┌──────────┴───────────┐
        ▼                      ▼
  ┌──────────┐          ┌──────────────┐
  │   ui/    │  render  │  highlight/  │  syntect + compose
  └────┬─────┘          └──────┬───────┘
       │ consume               │
       ▼                       ▼
            ┌──────────────┐
            │    diff/     │  FileDiff/Hunk/Line/Segment  (imara-diff engine)
            └──────┬───────┘
                   │ gets contents via
                   ▼
            ┌──────────────┐
            │    git/      │  GitBackend trait  ──impl──▶ git2_backend
            └──────────────┘
```

## The `GitBackend` trait

The single seam over version control. Everything above `git/` knows only this trait and its
DTOs — never `git2::*`. This is what lets us swap to `gix` later (ADR 0001). The rule is
**test-enforced**: `tests/architecture.rs` fails if any code outside `src/git/` references
`git2`.

```rust
pub trait GitBackend {
    /// Repo-relative root + current branch / head description for the header.
    fn context(&self) -> Result<RepoContext>;

    /// The list of changed files for a comparison (with status, rename info, +/- counts).
    fn changed_files(&self, spec: &CompareSpec) -> Result<Vec<FileChange>>;

    /// Old and new contents for one file under a comparison (None = absent side).
    fn file_contents(&self, spec: &CompareSpec, change: &FileChange)
        -> Result<(Option<Vec<u8>>, Option<Vec<u8>>)>;

    /// Resolve a ref / detect the base branch (merge-base) for smart mode.
    fn resolve(&self, rev: &str) -> Result<Oid>;
    fn detect_base(&self, candidates: &[String]) -> Result<Option<Rev>>;
}
```

PR resolution (`gh`) lives in `git/base.rs` and produces a `CompareSpec::Range`/`WorkdirVs`
the backend already understands — it is not part of the trait.

## Data flow (one refresh)

1. `app` resolves CLI/picker → `CompareSpec`.
2. `app::async_diff` asks `GitBackend::changed_files`, then for each file
   `file_contents`, on a worker thread.
3. `diff::engine` runs imara-diff on (old, new) → `FileDiff` (line hunks). For each changed
   line group it runs imara-diff again at word-token granularity → `Segment`s.
4. Result is sent over a crossbeam channel to the UI thread.
5. `highlight` syntax-highlights each visible line (cached per file); `compose` overlays
   syntax fg + diff bg + word-level bg into `ratatui` `Span`s.
6. `ui` lays out header / tree / viewer / keybar and renders the frame.
7. `app::watcher` (notify, debounced) re-triggers step 2 for working-tree comparisons.

## Why two imara-diff passes

imara-diff is generic over a token interner. Pass 1 interns **lines** → line-level hunks
(what's added/removed/context). Pass 2 interns **words** within an aligned removed/added line
pair → which spans inside the line changed. Same fast engine, two granularities. (ADR 0002.)

## Render purity

Rendering functions take immutable state and return `Vec<Line<'_>>` (or write to a `Buffer`),
with no I/O. That keeps them snapshot-testable on `TestBackend` without a terminal — the basis
of our headless verification (see `docs/testing.md`).
