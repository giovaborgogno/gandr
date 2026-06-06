//! Application state, the event loop, and key dispatch.

pub mod jobs;
pub mod watcher;

use crate::browser::Browser;
use crate::config::{Config, ViewMode};
use crate::diff::fold::{self, DiffRow};
use crate::diff::{engine, FileDiff, Line as DiffLine};
use crate::fuzzy;
use crate::git::{base, CompareSpec, GitBackend, RefEntry, RepoContext};
use crate::highlight::{FgSpan, Highlighter, ThemeMode};
use crate::review::{diff_hash, ReviewState, ReviewStatus};
use crate::search::{SearchMode, SearchResults};
use crate::ui;
use crate::ui::tree::{self, Row, RowKind};
use anyhow::{Context as _, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

/// Which pane the directional keys act on. Navigation is *hybrid*: `Tab` switches
/// focus (driving `j`/`k`), but `n`/`p` (file) and `]`/`[` (hunk) work regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Diff,
}

/// Top-level tab (gitui-style). `Diff` reviews changes; `Files` browses the repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Diff,
    Files,
}

/// What a compare-picker entry does when chosen.
#[derive(Debug, Clone)]
enum PickerAction {
    Spec(CompareSpec),
    /// Resolve the current branch's PR via `gh` on selection.
    Pr,
    /// Open the fuzzy ref picker (branches/tags) on selection.
    RefSearch,
}

/// One entry in the compare picker.
#[derive(Debug, Clone)]
pub struct PickerItem {
    pub label: String,
    action: PickerAction,
}

/// The compare-picker overlay state.
pub struct Picker {
    pub items: Vec<PickerItem>,
    pub selected: usize,
}

/// The fuzzy ref-picker overlay: pick any branch/tag to compare the working
/// tree against. `filtered` holds indices into `all`, ranked best-match-first.
pub struct RefPicker {
    pub query: String,
    pub all: Vec<RefEntry>,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

impl RefPicker {
    /// The currently highlighted ref entry, if any.
    pub fn current(&self) -> Option<&RefEntry> {
        self.filtered.get(self.selected).map(|&i| &self.all[i])
    }
}

/// The whole application state.
pub struct App {
    config: Config,
    backend: Box<dyn GitBackend>,
    view: ViewMode,
    focus: Focus,
    context: RepoContext,
    spec: CompareSpec,
    /// Optional header label overriding the spec's (e.g. a PR title).
    title: Option<String>,
    /// Last error (e.g. a bad ref / failed `gh`), shown in the keybar.
    error: Option<String>,
    files: Vec<FileDiff>,
    /// Index into `files` of the selected file (the one shown in the viewer).
    selected: usize,
    /// First visible row of the diff viewer for the selected file.
    scroll: usize,
    /// Collapsed directory paths in the file tree (absent = expanded).
    collapsed: HashSet<PathBuf>,
    /// Cursor over the tree's visible rows (dirs + files).
    tree_cursor: usize,
    /// First visible tree row (follows the cursor); updated at render.
    tree_scroll: Cell<usize>,
    /// Diff-viewport height from the last render, for page/bottom math.
    viewport: Cell<usize>,
    /// Cached stateful syntax-highlight spans for the selected file's old/new
    /// sides (M12), keyed by (path, theme). Recomputed lazily on render when the
    /// selection or theme changes — never per frame.
    diff_hl: RefCell<DiffHighlight>,
    /// Full annotated line list of the selected file (no folding), cached by
    /// path. Feeds the folded display rows and per-gap expand.
    full_cache: RefCell<(Option<PathBuf>, Rc<Vec<DiffLine>>)>,
    /// Cached folded display rows, keyed by (path, base context, expand version).
    display_cache: RefCell<DisplayRows>,
    /// Per-gap reveal state for the file at `expanded_path`: maps a fold's anchor
    /// (its run start index) to how many lines the user has revealed from the top.
    /// Reset when the selected file or its content changes.
    expanded_folds: HashMap<usize, usize>,
    expanded_path: Option<PathBuf>,
    /// Bumped on each expand so the display cache recomputes.
    expanded_version: u64,
    /// The compare picker overlay, when open.
    picker: Option<Picker>,
    /// Persisted review state + where it lives.
    review: ReviewState,
    review_path: PathBuf,
    /// Cached per-file review status (recomputed on refresh/toggle/spec change,
    /// not every frame — diff hashing is O(diff size)).
    review_cache: Vec<ReviewStatus>,
    /// Whether file changes auto-refresh the diff (working-tree comparisons).
    auto_refresh: bool,
    /// Monotonic token; a background diff result is applied only if it matches.
    refresh_epoch: u64,
    /// A queued async refresh (epoch) the event loop should spawn.
    pending_refresh: Option<u64>,
    /// Whether a background recompute is in flight (shown in the header).
    loading: bool,
    /// Light/dark rendering mode (resolved from config/terminal in `run`).
    theme_mode: ThemeMode,
    /// The active top-level tab.
    tab: Tab,
    /// The Files-tab repo browser.
    browser: Browser,
    /// Whether the help overlay is shown.
    show_help: bool,
    /// In-diff text search state (open when `Some`).
    search: Option<Search>,
    /// The fuzzy ref-picker overlay, when open.
    ref_picker: Option<RefPicker>,
    /// Repo-wide search overlay (Files tab), open when `Some`.
    repo_search: Option<RepoSearch>,
    /// Monotonic token; a background search result is applied only if it matches.
    search_epoch: u64,
    /// A queued async search the event loop should spawn: (epoch, query, mode).
    pending_search: Option<(u64, String, SearchMode)>,
    /// A pending "open in editor" request (path, 1-based line), taken by run_loop.
    editor_request: Option<(PathBuf, u32)>,
    should_quit: bool,
}

/// Repo-wide search (Files tab): a live query over file names or contents, with
/// a results list you navigate (↑/↓) and jump into (Enter). `Tab` flips the mode.
pub struct RepoSearch {
    pub query: String,
    pub mode: SearchMode,
    pub results: SearchResults,
    pub selected: usize,
    /// Whether a background search for the current query is in flight.
    pub loading: bool,
}

/// Shared, per-line syntax-highlight spans for one side of a file (one span list
/// per line). Behind an `Rc` so the render path can clone the handle cheaply.
pub type SideHighlight = Rc<Vec<Vec<FgSpan>>>;

/// Cached per-line syntax highlighting for the selected file's two sides,
/// computed with carried state across the whole file (M12). `old`/`new` are
/// indexed by `old_no - 1` / `new_no - 1`.
#[derive(Default)]
struct DiffHighlight {
    key: Option<(PathBuf, ThemeMode)>,
    old: SideHighlight,
    new: SideHighlight,
}

/// Cached folded display rows for the selected file, with the key they were
/// computed for (path, base context lines, expand version).
#[derive(Default)]
struct DisplayRows {
    key: Option<(PathBuf, usize, u64)>,
    rows: Rc<Vec<DiffRow>>,
}

/// In-diff search: the query and the current match's row offset.
pub struct Search {
    pub query: String,
    /// Whether we're still typing the query (vs navigating matches).
    pub editing: bool,
}

impl App {
    /// Build the app: opens the comparison via `backend` and computes its diff.
    pub fn new(config: Config, backend: Box<dyn GitBackend>, spec: CompareSpec) -> Result<Self> {
        Self::with_title(config, backend, spec, None)
    }

    /// Like [`App::new`] but with an explicit header title (e.g. a PR title).
    pub fn with_title(
        config: Config,
        backend: Box<dyn GitBackend>,
        spec: CompareSpec,
        title: Option<String>,
    ) -> Result<Self> {
        let context = backend.context()?;
        let files = engine::build_diffs(backend.as_ref(), &spec, config.context_lines)?;
        let view = config.default_view;
        let review_path = ReviewState::state_path(&context.root);
        let review = ReviewState::load(&review_path);
        let auto_refresh = config.auto_refresh;
        let browser = Browser::new(context.root.clone());

        let mut app = Self {
            config,
            backend,
            view,
            focus: Focus::Tree,
            context,
            spec,
            title,
            error: None,
            files,
            selected: 0,
            scroll: 0,
            collapsed: HashSet::new(),
            tree_cursor: 0,
            tree_scroll: Cell::new(0),
            viewport: Cell::new(0),
            diff_hl: RefCell::new(DiffHighlight::default()),
            full_cache: RefCell::new((None, Rc::new(Vec::new()))),
            display_cache: RefCell::new(DisplayRows::default()),
            expanded_folds: HashMap::new(),
            expanded_path: None,
            expanded_version: 0,
            picker: None,
            review,
            review_path,
            review_cache: Vec::new(),
            auto_refresh,
            refresh_epoch: 0,
            pending_refresh: None,
            loading: false,
            theme_mode: ThemeMode::Dark,
            tab: Tab::Diff,
            browser,
            show_help: false,
            search: None,
            ref_picker: None,
            repo_search: None,
            search_epoch: 0,
            pending_search: None,
            editor_request: None,
            should_quit: false,
        };
        app.reset_view();
        app.recompute_review();
        Ok(app)
    }

    // ---- accessors used by the ui layer ----

    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn view(&self) -> ViewMode {
        self.view
    }
    pub fn focus(&self) -> Focus {
        self.focus
    }
    pub fn context(&self) -> &RepoContext {
        &self.context
    }
    pub fn files(&self) -> &[FileDiff] {
        &self.files
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn scroll(&self) -> usize {
        self.scroll
    }
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }
    pub fn auto_refresh(&self) -> bool {
        self.auto_refresh
    }
    /// Whether the current comparison watches the working tree.
    pub fn spec_is_live(&self) -> bool {
        self.spec.is_live()
    }
    pub fn theme_mode(&self) -> ThemeMode {
        self.theme_mode
    }
    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme_mode = mode;
        // Keep the Files-tab content highlighting (computed at load) in sync.
        self.browser.set_mode(mode);
    }
    pub fn tab(&self) -> Tab {
        self.tab
    }
    pub fn browser(&self) -> &Browser {
        &self.browser
    }

    /// The comparison key under which review state is stored.
    fn review_key(&self) -> String {
        self.spec.label()
    }

    /// Per-file review status, indexed to match `files()` (from the cache).
    pub fn review_statuses(&self) -> &[ReviewStatus] {
        &self.review_cache
    }

    /// Recompute the cached review status for every file.
    fn recompute_review(&mut self) {
        let key = self.review_key();
        self.review_cache = self
            .files
            .iter()
            .map(|f| self.review.status(&key, &f.change.path, diff_hash(f)))
            .collect();
    }

    /// Number of files marked reviewed (for the panel title).
    pub fn reviewed_count(&self) -> usize {
        self.review_cache
            .iter()
            .filter(|s| **s == ReviewStatus::Reviewed)
            .count()
    }

    /// The currently selected file's diff, if any.
    pub fn current(&self) -> Option<&FileDiff> {
        self.files.get(self.selected)
    }

    /// Total additions / deletions across all files, for the header.
    pub fn totals(&self) -> (usize, usize) {
        self.files.iter().fold((0, 0), |(a, d), f| {
            (a + f.change.additions, d + f.change.deletions)
        })
    }

    /// The current branch (or `(detached)`), for the header.
    pub fn branch(&self) -> &str {
        self.context.branch.as_deref().unwrap_or("(detached)")
    }

    /// The comparison label (a PR title if set, else the spec's label).
    pub fn comparison_label(&self) -> String {
        self.title.clone().unwrap_or_else(|| self.spec.label())
    }

    /// Whether the working tree is being watched (live comparison + auto-refresh).
    pub fn is_watching(&self) -> bool {
        self.auto_refresh && self.spec.is_live()
    }

    /// The active context-fold window (3 = default/tightest).
    pub fn context_lines(&self) -> usize {
        self.config.context_lines
    }

    /// A complete one-line state summary (branch, comparison, totals, reviewed,
    /// indicators). The header renders a compact subset; this stays the canonical
    /// summary used in tests/debugging.
    pub fn header_line(&self) -> String {
        let (add, del) = self.totals();
        let n = self.files.len();
        let files = if n == 1 { "file" } else { "files" };
        let reviewed = self.reviewed_count();
        let watch = if self.is_watching() {
            " · ◉ watching"
        } else {
            ""
        };
        let load = if self.loading { " · ⟳ loading" } else { "" };
        let ctx = if self.config.context_lines > 3 {
            format!(" · ⊕{} ctx", self.config.context_lines)
        } else {
            String::new()
        };
        format!(
            "{} · {} · {n} {files}  +{add} −{del} · {reviewed}/{n} reviewed{watch}{ctx}{load}",
            self.branch(),
            self.comparison_label(),
        )
    }

    /// The last error to surface in the keybar, if any.
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Let the renderer record the diff-viewport height for page math.
    pub fn set_viewport(&self, rows: usize) {
        self.viewport.set(rows);
    }

    /// Stateful syntax-highlight spans for the selected file's old and new sides
    /// (indexed by `old_no-1` / `new_no-1`). Computed once per (file, theme) and
    /// cached; the `Rc` clones are cheap, so the renderer can call this per frame.
    pub fn diff_highlight(&self) -> (SideHighlight, SideHighlight) {
        let key = self
            .current()
            .map(|f| (f.change.path.clone(), self.theme_mode));
        if self.diff_hl.borrow().key == key {
            let cache = self.diff_hl.borrow();
            return (cache.old.clone(), cache.new.clone());
        }
        let (old, new) = match self.current() {
            Some(f) => {
                let hl = Highlighter::for_path(&f.change.path, self.theme_mode);
                (
                    Rc::new(hl.highlight_file(&engine::split_lines(&f.old_text))),
                    Rc::new(hl.highlight_file(&engine::split_lines(&f.new_text))),
                )
            }
            None => (Rc::new(Vec::new()), Rc::new(Vec::new())),
        };
        let mut cache = self.diff_hl.borrow_mut();
        cache.key = key;
        cache.old = Rc::clone(&old);
        cache.new = Rc::clone(&new);
        (old, new)
    }

    /// The full annotated line list of the selected file (no folding), cached by
    /// path. Empty when nothing is selected.
    pub fn full_lines(&self) -> Rc<Vec<DiffLine>> {
        let path = self.current().map(|f| f.change.path.clone());
        if self.full_cache.borrow().0 == path {
            return self.full_cache.borrow().1.clone();
        }
        let full = match self.current() {
            Some(f) => Rc::new(engine::all_lines(&f.old_text, &f.new_text)),
            None => Rc::new(Vec::new()),
        };
        *self.full_cache.borrow_mut() = (path, Rc::clone(&full));
        full
    }

    /// The folded display rows for the selected file: changed lines plus base
    /// context, with longer unchanged runs collapsed to folds (expanded ones
    /// shown in full). Cached by (path, context, expand version).
    pub fn display_rows(&self) -> Rc<Vec<DiffRow>> {
        let path = self.current().map(|f| f.change.path.clone());
        let context = self.config.context_lines;
        let key = path.clone().map(|p| (p, context, self.expanded_version));
        if self.display_cache.borrow().key == key {
            return self.display_cache.borrow().rows.clone();
        }
        let full = self.full_lines();
        // Only apply expansions that belong to the currently selected file.
        let empty = HashMap::new();
        let expanded = if self.expanded_path == path {
            &self.expanded_folds
        } else {
            &empty
        };
        let rows = Rc::new(fold::fold(&full, context, expanded));
        *self.display_cache.borrow_mut() = DisplayRows {
            key,
            rows: Rc::clone(&rows),
        };
        rows
    }

    /// Drop the per-file render caches (full lines, folded rows, highlight) and
    /// fold expansions. Called when the file set changes so a same-path edit
    /// doesn't reuse stale content.
    fn invalidate_file_caches(&mut self) {
        self.diff_hl.get_mut().key = None;
        self.full_cache.get_mut().0 = None;
        self.display_cache.get_mut().key = None;
        self.expanded_folds.clear();
        self.expanded_path = None;
        self.expanded_version = self.expanded_version.wrapping_add(1);
    }

    /// How many lines each `Enter` reveals from the top of the active fold.
    const EXPAND_STEP: usize = 10;

    /// Reveal more of the fold nearest the top of the viewport (the gap the user
    /// is looking at): each call shows another [`Self::EXPAND_STEP`] lines from
    /// its top. Repeating walks down the gap; once a gap is fully shown the next
    /// call targets the following one.
    fn expand_active_fold(&mut self) {
        let rows = self.display_rows();
        let folds: Vec<(usize, usize)> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                DiffRow::Fold { anchor, .. } => Some((i, *anchor)),
                DiffRow::Line(_) => None,
            })
            .collect();
        if folds.is_empty() {
            return;
        }
        // The fold at or after the current scroll (the gap below the top of the
        // viewport); if we're past every fold, the nearest one above (the last).
        let (_, anchor) = folds
            .iter()
            .find(|(row, _)| *row >= self.scroll)
            .copied()
            .unwrap_or_else(|| folds[folds.len() - 1]);

        let path = self.current().map(|f| f.change.path.clone());
        if self.expanded_path != path {
            self.expanded_path = path;
            self.expanded_folds.clear();
        }
        *self.expanded_folds.entry(anchor).or_insert(0) += Self::EXPAND_STEP;
        self.expanded_version = self.expanded_version.wrapping_add(1);
    }

    // ---- file tree ----

    /// The current visible tree rows (dirs + files, compacted + collapse-aware).
    pub fn tree_rows(&self) -> Vec<Row> {
        tree::build_rows(&self.files, &self.collapsed)
    }

    /// The cursor row in the tree.
    pub fn tree_cursor(&self) -> usize {
        self.tree_cursor
    }

    /// First visible tree row so the cursor stays on-screen for `height` rows.
    pub fn tree_scroll(&self, height: usize) -> usize {
        let mut s = self.tree_scroll.get();
        if self.tree_cursor < s {
            s = self.tree_cursor;
        } else if height > 0 && self.tree_cursor >= s + height {
            s = self.tree_cursor + 1 - height;
        }
        self.tree_scroll.set(s);
        s
    }

    // ---- diff-row geometry (over the folded display rows) ----

    /// Row offsets of each visible region's first line (after each fold, plus the
    /// top) — the jump targets for `]`/`[`.
    fn hunk_offsets(&self) -> Vec<usize> {
        let rows = self.display_rows();
        let mut offsets = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            if matches!(row, DiffRow::Line(_)) {
                let region_start = i == 0 || matches!(rows[i - 1], DiffRow::Fold { .. });
                if region_start {
                    offsets.push(i);
                }
            }
        }
        offsets
    }

    /// Total viewer rows for the selected file (folded display rows).
    fn total_rows(&self) -> usize {
        self.display_rows().len()
    }

    fn max_scroll(&self) -> usize {
        self.total_rows().saturating_sub(self.viewport.get().max(1))
    }

    // ---- mutations ----

    /// Point the viewer at the file under the tree cursor (if it's a file row).
    fn sync_selection(&mut self) {
        let rows = self.tree_rows();
        if let Some(idx) = rows.get(self.tree_cursor).and_then(Row::file_index) {
            if idx != self.selected {
                self.selected = idx;
                self.scroll = 0;
            }
        }
    }

    fn cursor_down(&mut self) {
        let len = self.tree_rows().len();
        if len == 0 {
            return;
        }
        self.tree_cursor = (self.tree_cursor + 1).min(len - 1);
        self.sync_selection();
    }

    fn cursor_up(&mut self) {
        self.tree_cursor = self.tree_cursor.saturating_sub(1);
        self.sync_selection();
    }

    /// Jump the cursor to the next/previous file row (skipping directories).
    fn select_next_file(&mut self) {
        let rows = self.tree_rows();
        if let Some(pos) = rows
            .iter()
            .enumerate()
            .skip(self.tree_cursor + 1)
            .find(|(_, r)| r.file_index().is_some())
            .map(|(i, _)| i)
        {
            self.tree_cursor = pos;
            self.sync_selection();
        }
    }

    fn select_prev_file(&mut self) {
        let rows = self.tree_rows();
        if let Some(pos) = rows
            .iter()
            .enumerate()
            .take(self.tree_cursor)
            .rev()
            .find(|(_, r)| r.file_index().is_some())
            .map(|(i, _)| i)
        {
            self.tree_cursor = pos;
            self.sync_selection();
        }
    }

    /// Toggle the directory under the cursor; collapse/expand explicitly.
    fn set_dir_collapsed(&mut self, collapse: bool) {
        let rows = self.tree_rows();
        if let Some(Row {
            kind: RowKind::Dir { path, expanded },
            ..
        }) = rows.get(self.tree_cursor)
        {
            if collapse && *expanded {
                self.collapsed.insert(path.clone());
            } else if !collapse && !*expanded {
                self.collapsed.remove(path);
            }
            self.clamp_cursor();
        }
    }

    fn toggle_dir(&mut self) {
        let rows = self.tree_rows();
        if let Some(Row {
            kind: RowKind::Dir { path, expanded },
            ..
        }) = rows.get(self.tree_cursor)
        {
            if *expanded {
                self.collapsed.insert(path.clone());
            } else {
                self.collapsed.remove(path);
            }
            self.clamp_cursor();
        }
    }

    fn clamp_cursor(&mut self) {
        let len = self.tree_rows().len();
        if len > 0 && self.tree_cursor >= len {
            self.tree_cursor = len - 1;
        }
    }

    fn scroll_down(&mut self, n: usize) {
        self.scroll = (self.scroll + n).min(self.max_scroll());
    }

    fn scroll_up(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    /// Cycle how many context lines surround each hunk (expand, then wrap back to
    /// the tightest view). The viewer folds from the full line list, and the
    /// display-row cache is keyed by context — so this just changes the base
    /// context; no diff rebuild or git access is needed (it also preserves any
    /// per-gap expansions). `git diff -U<n>`-style.
    fn cycle_context(&mut self) {
        const LADDER: [usize; 4] = [3, 10, 30, 100];
        let cur = self.config.context_lines;
        self.config.context_lines = LADDER
            .iter()
            .copied()
            .find(|&c| c > cur)
            .unwrap_or(LADDER[0]);
        // Re-clamp scroll in case fewer rows are shown after re-folding.
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn next_hunk(&mut self) {
        if let Some(&off) = self.hunk_offsets().iter().find(|&&o| o > self.scroll) {
            self.scroll = off.min(self.max_scroll());
        }
    }

    fn prev_hunk(&mut self) {
        if let Some(&off) = self.hunk_offsets().iter().rev().find(|&&o| o < self.scroll) {
            self.scroll = off;
        }
    }

    // ---- comparison & picker ----

    /// Reset selection/cursor/scroll to the first file after the file set changes.
    fn reset_view(&mut self) {
        let rows = self.tree_rows();
        self.tree_cursor = rows
            .iter()
            .position(|r| r.file_index().is_some())
            .unwrap_or(0);
        self.selected = rows
            .get(self.tree_cursor)
            .and_then(Row::file_index)
            .unwrap_or(0);
        self.scroll = 0;
        self.tree_scroll.set(0);
    }

    /// Switch the comparison and recompute the diff asynchronously (the event
    /// loop spawns the job; the result replaces the file set when it arrives).
    fn set_spec(&mut self, spec: CompareSpec, title: Option<String>) {
        self.spec = spec;
        self.title = title;
        self.collapsed.clear();
        self.request_refresh();
    }

    /// Apply a freshly-computed file set, preserving the selected file (by path)
    /// and scroll where possible.
    fn apply_files(&mut self, files: Vec<FileDiff>) {
        let prev_path = self.current().map(|f| f.change.path.clone());
        let prev_scroll = self.scroll;
        self.files = files;
        self.error = None;
        // The file set (and thus old/new text) changed: drop per-file caches so a
        // same-path working-tree edit doesn't reuse stale content/colors/folds.
        self.invalidate_file_caches();
        if let Some(idx) =
            prev_path.and_then(|p| self.files.iter().position(|f| f.change.path == p))
        {
            self.selected = idx;
            if let Some(c) = self
                .tree_rows()
                .iter()
                .position(|r| r.file_index() == Some(idx))
            {
                self.tree_cursor = c;
            }
            self.scroll = prev_scroll.min(self.max_scroll());
        } else {
            self.reset_view();
        }
        self.recompute_review();
    }

    /// Synchronously recompute the diff (used in tests and as a fallback).
    pub fn refresh(&mut self) {
        match engine::build_diffs(self.backend.as_ref(), &self.spec, self.config.context_lines) {
            Ok(files) => self.apply_files(files),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Request an async refresh (the event loop spawns the background job).
    pub fn request_refresh(&mut self) {
        self.refresh_epoch += 1;
        self.pending_refresh = Some(self.refresh_epoch);
        self.loading = true;
    }

    /// Take a queued refresh request: returns (epoch, spec) for the loop to spawn.
    pub fn take_pending_refresh(&mut self) -> Option<(u64, CompareSpec)> {
        self.pending_refresh.take().map(|e| (e, self.spec.clone()))
    }

    /// Apply a background diff result, ignoring it if a newer refresh superseded it.
    pub fn apply_diff_result(&mut self, epoch: u64, files: Result<Vec<FileDiff>>) {
        if epoch != self.refresh_epoch {
            return; // stale; a newer refresh is in flight
        }
        self.loading = false;
        match files {
            Ok(files) => self.apply_files(files),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Toggle review of the selected file and persist.
    fn toggle_review(&mut self) {
        let info = self
            .current()
            .map(|f| (f.change.path.clone(), diff_hash(f)));
        if let Some((path, hash)) = info {
            let key = self.review_key();
            self.review.toggle(&key, &path, hash);
            if let Err(e) = self.review.save(&self.review_path) {
                self.error = Some(format!("save review state: {e}"));
            }
            self.recompute_review();
        }
    }

    /// The open compare picker, if any (for rendering).
    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    pub fn show_help(&self) -> bool {
        self.show_help
    }
    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }
    pub fn repo_search(&self) -> Option<&RepoSearch> {
        self.repo_search.as_ref()
    }
    pub fn ref_picker(&self) -> Option<&RefPicker> {
        self.ref_picker.as_ref()
    }
    /// Take a pending editor request (path, 1-based line), if any.
    pub fn take_editor_request(&mut self) -> Option<(PathBuf, u32)> {
        self.editor_request.take()
    }

    /// Queue opening the selected file in `$EDITOR` near its first change.
    fn open_editor(&mut self) {
        if let Some(file) = self.current() {
            let line = file.hunks.first().map(|h| h.new_start).unwrap_or(1);
            let path = self.context.root.join(&file.change.path);
            self.editor_request = Some((path, line));
        }
    }

    /// Display-row offsets of lines matching the search query (folds skipped).
    fn search_matches(&self) -> Vec<usize> {
        let query = match &self.search {
            Some(s) if !s.query.is_empty() => s.query.to_lowercase(),
            _ => return Vec::new(),
        };
        let rows = self.display_rows();
        let full = self.full_lines();
        rows.iter()
            .enumerate()
            .filter_map(|(i, row)| match row {
                DiffRow::Line(idx) if full[*idx].text.to_lowercase().contains(&query) => Some(i),
                _ => None,
            })
            .collect()
    }

    /// Jump the viewer to the next/previous search match (wrapping).
    fn search_jump(&mut self, forward: bool) {
        let matches = self.search_matches();
        if matches.is_empty() {
            return;
        }
        let cur = self.scroll;
        let target = if forward {
            matches
                .iter()
                .find(|&&o| o > cur)
                .or_else(|| matches.first())
        } else {
            matches
                .iter()
                .rev()
                .find(|&&o| o < cur)
                .or_else(|| matches.last())
        };
        if let Some(&off) = target {
            self.scroll = off.min(self.max_scroll());
        }
    }

    /// Route a key to search mode; returns whether it was handled.
    fn search_key(&mut self, key: KeyEvent) -> bool {
        let Some(search) = self.search.as_mut() else {
            return false;
        };
        if search.editing {
            match key.code {
                KeyCode::Char(c) => search.query.push(c),
                KeyCode::Backspace => {
                    search.query.pop();
                }
                KeyCode::Enter => {
                    search.editing = false;
                    self.search_jump(true);
                }
                KeyCode::Esc => self.search = None,
                _ => {}
            }
            true
        } else {
            // Navigating matches: n/N move, Esc closes; other keys pass through.
            match key.code {
                KeyCode::Char('n') => {
                    self.search_jump(true);
                    true
                }
                KeyCode::Char('N') => {
                    self.search_jump(false);
                    true
                }
                KeyCode::Esc => {
                    self.search = None;
                    true
                }
                _ => false,
            }
        }
    }

    // ---- repo-wide search (Files tab) ----

    /// Empty results matching `mode` (so the overlay knows which list it shows).
    fn empty_results(mode: SearchMode) -> SearchResults {
        match mode {
            SearchMode::Files => SearchResults::Files(Vec::new()),
            SearchMode::Content => SearchResults::Content(Vec::new()),
        }
    }

    /// Open the repo-wide search overlay (defaults to content search).
    fn open_repo_search(&mut self) {
        let mode = SearchMode::Content;
        self.repo_search = Some(RepoSearch {
            query: String::new(),
            mode,
            results: Self::empty_results(mode),
            selected: 0,
            loading: false,
        });
    }

    /// Queue an async search for the current query/mode (or clear results when
    /// the query is empty). The event loop spawns the background job.
    fn request_search(&mut self) {
        let Some(rs) = self.repo_search.as_ref() else {
            return;
        };
        let query = rs.query.clone();
        let mode = rs.mode;
        self.search_epoch += 1;
        let epoch = self.search_epoch;
        if query.trim().is_empty() {
            self.pending_search = None;
            if let Some(rs) = self.repo_search.as_mut() {
                rs.results = Self::empty_results(mode);
                rs.loading = false;
                rs.selected = 0;
            }
            return;
        }
        self.pending_search = Some((epoch, query, mode));
        if let Some(rs) = self.repo_search.as_mut() {
            rs.loading = true;
        }
    }

    /// Take a queued search request for the loop to spawn.
    pub fn take_pending_search(&mut self) -> Option<(u64, String, SearchMode)> {
        self.pending_search.take()
    }

    /// Apply a background search result, ignoring it if a newer query superseded it.
    pub fn apply_search_result(&mut self, epoch: u64, results: SearchResults) {
        if epoch != self.search_epoch {
            return; // stale; a newer query is in flight
        }
        if let Some(rs) = self.repo_search.as_mut() {
            rs.results = results;
            rs.loading = false;
            rs.selected = 0;
        }
    }

    /// Move the selection within the results list.
    fn result_move(&mut self, down: bool) {
        if let Some(rs) = self.repo_search.as_mut() {
            let len = rs.results.len();
            if len == 0 {
                return;
            }
            rs.selected = if down {
                (rs.selected + 1).min(len - 1)
            } else {
                rs.selected.saturating_sub(1)
            };
        }
    }

    /// Jump to the selected result: reveal it in the browser (at its line for a
    /// content hit), focus the content pane, and close the overlay.
    fn jump_to_result(&mut self) {
        let root = self.context.root.clone();
        let target = self.repo_search.as_ref().and_then(|rs| match &rs.results {
            SearchResults::Files(v) => v.get(rs.selected).map(|p| (root.join(p), None)),
            SearchResults::Content(v) => v
                .get(rs.selected)
                .map(|m| (root.join(&m.path), Some(m.line as usize))),
        });
        if let Some((abs, line)) = target {
            self.browser.reveal(&abs, line);
            self.focus = Focus::Diff; // focus the content pane
            self.repo_search = None;
            self.pending_search = None;
        }
    }

    /// Route a key to the open repo-search overlay; returns whether it was handled.
    fn repo_search_key(&mut self, key: KeyEvent) -> bool {
        if self.repo_search.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.repo_search = None;
                self.pending_search = None;
            }
            KeyCode::Tab => {
                if let Some(rs) = self.repo_search.as_mut() {
                    rs.mode = rs.mode.toggled();
                }
                self.request_search();
            }
            KeyCode::Up => self.result_move(false),
            KeyCode::Down => self.result_move(true),
            KeyCode::Enter => self.jump_to_result(),
            KeyCode::Backspace => {
                if let Some(rs) = self.repo_search.as_mut() {
                    rs.query.pop();
                }
                self.request_search();
            }
            KeyCode::Char(c) => {
                if let Some(rs) = self.repo_search.as_mut() {
                    rs.query.push(c);
                }
                self.request_search();
            }
            _ => {}
        }
        true
    }

    // ---- fuzzy ref picker ----

    /// Open the fuzzy ref picker, loading branches/tags from the backend. On a
    /// backend error the picker stays closed and the error surfaces in the keybar.
    fn open_ref_picker(&mut self) {
        match self.backend.list_refs() {
            Ok(all) => {
                let filtered = (0..all.len()).collect();
                self.ref_picker = Some(RefPicker {
                    query: String::new(),
                    all,
                    filtered,
                    selected: 0,
                });
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Recompute the ranked `filtered` list from the current query.
    fn ref_picker_filter(&mut self) {
        let Some(rp) = self.ref_picker.as_mut() else {
            return;
        };
        if rp.query.is_empty() {
            rp.filtered = (0..rp.all.len()).collect();
        } else {
            let mut scored: Vec<(usize, i32)> = rp
                .all
                .iter()
                .enumerate()
                .filter_map(|(i, e)| fuzzy::score(&rp.query, &e.name).map(|s| (i, s)))
                .collect();
            // Best score first; ties broken by name for a stable, readable order.
            scored.sort_by(|a, b| b.1.cmp(&a.1).then(rp.all[a.0].name.cmp(&rp.all[b.0].name)));
            rp.filtered = scored.into_iter().map(|(i, _)| i).collect();
        }
        // On an empty result set `selected` lands at 0 (a non-existent row); all
        // consumers (`current`, render, apply) read it through `.get()`/iteration,
        // so this never indexes out of bounds.
        rp.selected = rp.selected.min(rp.filtered.len().saturating_sub(1));
    }

    /// Apply the highlighted ref: compare the working tree against it.
    fn ref_picker_apply(&mut self) {
        let name = self
            .ref_picker
            .as_ref()
            .and_then(|rp| rp.current())
            .map(|e| e.name.clone());
        if let Some(name) = name {
            self.ref_picker = None;
            self.set_spec(CompareSpec::WorkdirVs(name), None);
        }
    }

    /// Route a key to the open ref picker; returns whether it was handled.
    fn ref_picker_key(&mut self, key: KeyEvent) -> bool {
        if self.ref_picker.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Esc => self.ref_picker = None,
            KeyCode::Enter => self.ref_picker_apply(),
            KeyCode::Up => {
                if let Some(rp) = self.ref_picker.as_mut() {
                    rp.selected = rp.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(rp) = self.ref_picker.as_mut() {
                    if rp.selected + 1 < rp.filtered.len() {
                        rp.selected += 1;
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(rp) = self.ref_picker.as_mut() {
                    rp.query.pop();
                }
                self.ref_picker_filter();
            }
            KeyCode::Char(c) => {
                if let Some(rp) = self.ref_picker.as_mut() {
                    rp.query.push(c);
                }
                self.ref_picker_filter();
            }
            _ => {}
        }
        true
    }

    fn open_picker(&mut self) {
        let mut items = vec![
            PickerItem {
                label: "Uncommitted (vs HEAD)".into(),
                action: PickerAction::Spec(CompareSpec::Uncommitted),
            },
            PickerItem {
                label: "Staged (index vs HEAD)".into(),
                action: PickerAction::Spec(CompareSpec::Staged),
            },
        ];
        if let Ok(Some(base)) = self.backend.detect_base(&self.config.base_branches) {
            items.push(PickerItem {
                label: "Branch vs base".into(),
                action: PickerAction::Spec(CompareSpec::WorkdirVs(base)),
            });
        }
        items.push(PickerItem {
            label: "Branch / tag… (fuzzy search)".into(),
            action: PickerAction::RefSearch,
        });
        items.push(PickerItem {
            label: "PR (current branch, via gh)".into(),
            action: PickerAction::Pr,
        });
        self.picker = Some(Picker { items, selected: 0 });
    }

    fn picker_apply(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        let Some(item) = picker.items.into_iter().nth(picker.selected) else {
            return;
        };
        match item.action {
            PickerAction::Spec(spec) => self.set_spec(spec, None),
            PickerAction::RefSearch => self.open_ref_picker(),
            PickerAction::Pr => match base::resolve_pr(None) {
                Ok(r) => self.set_spec(r.spec, r.title),
                Err(e) => self.error = Some(e.to_string()),
            },
        }
    }

    /// Route a key to the open picker; returns whether it was handled.
    fn picker_key(&mut self, key: KeyEvent) -> bool {
        let Some(picker) = self.picker.as_mut() else {
            return false;
        };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if picker.selected + 1 < picker.items.len() {
                    picker.selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                picker.selected = picker.selected.saturating_sub(1);
            }
            KeyCode::Enter => self.picker_apply(),
            KeyCode::Esc | KeyCode::Char('q') => self.picker = None,
            _ => {}
        }
        true
    }

    // ---- rendering & input ----

    pub fn render(&self, f: &mut Frame) {
        ui::render(self, f);
    }

    /// Handle a single key event. Pure state transition — easy to drive in tests.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Ctrl-C always quits, before anything else captures keys.
        if ctrl && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        // The help overlay, when shown, swallows the next key (any key closes it).
        if self.show_help {
            self.show_help = false;
            return;
        }

        // The repo-search overlay (Files tab) captures all keys while open, so
        // typing the query isn't intercepted by the global single-key shortcuts.
        if self.repo_search_key(key) {
            return;
        }
        // Likewise the fuzzy ref picker captures all keys while typing a query.
        if self.ref_picker_key(key) {
            return;
        }

        // Global keys (both tabs): quit, help, tab switching.
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('?') => {
                self.show_help = true;
                return;
            }
            KeyCode::Char('1') => {
                self.tab = Tab::Diff;
                self.picker = None;
                self.search = None;
                self.repo_search = None;
                self.pending_search = None;
                return;
            }
            KeyCode::Char('2') => {
                self.tab = Tab::Files;
                self.picker = None;
                self.search = None;
                self.ref_picker = None;
                self.repo_search = None;
                self.pending_search = None;
                return;
            }
            _ => {}
        }

        if self.tab == Tab::Files {
            self.handle_files_key(key, ctrl);
            return;
        }

        // ---- Diff tab ----
        // Search mode captures input while editing / navigating matches.
        if self.search_key(key) {
            return;
        }
        // The compare picker, when open, captures all other keys.
        if self.picker_key(key) {
            return;
        }
        let half_page = (self.viewport.get() / 2).max(1);

        match key.code {
            KeyCode::Char('/') => {
                self.search = Some(Search {
                    query: String::new(),
                    editing: true,
                })
            }
            KeyCode::Char('e') => self.open_editor(),
            KeyCode::Char('c') => self.open_picker(),
            KeyCode::Char('b') => self.open_ref_picker(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Tree => Focus::Diff,
                    Focus::Diff => Focus::Tree,
                }
            }
            KeyCode::Char('s') => self.view = self.view.toggled(),
            KeyCode::Char('w') => self.config.word_diff = !self.config.word_diff,
            KeyCode::Char('o') => self.cycle_context(),
            KeyCode::Char(' ') => self.toggle_review(),
            KeyCode::Char('r') => self.request_refresh(),
            KeyCode::Char('a') => self.auto_refresh = !self.auto_refresh,

            KeyCode::Char('d') if ctrl => self.scroll_down(half_page),
            KeyCode::Char('u') if ctrl => self.scroll_up(half_page),

            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                Focus::Tree => self.cursor_down(),
                Focus::Diff => self.scroll_down(1),
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                Focus::Tree => self.cursor_up(),
                Focus::Diff => self.scroll_up(1),
            },

            // Tree expand/collapse; Enter on a file focuses the diff.
            KeyCode::Enter if self.focus == Focus::Tree => {
                match self.tree_rows().get(self.tree_cursor).map(|r| &r.kind) {
                    Some(RowKind::Dir { .. }) => self.toggle_dir(),
                    Some(RowKind::File { .. }) => self.focus = Focus::Diff,
                    None => {}
                }
            }
            // In the diff, Enter expands the fold nearest the top of the viewport.
            KeyCode::Enter if self.focus == Focus::Diff => self.expand_active_fold(),
            KeyCode::Right if self.focus == Focus::Tree => self.set_dir_collapsed(false),
            KeyCode::Left if self.focus == Focus::Tree => self.set_dir_collapsed(true),

            KeyCode::Char('n') => self.select_next_file(),
            KeyCode::Char('p') => self.select_prev_file(),
            KeyCode::Char(']') => self.next_hunk(),
            KeyCode::Char('[') => self.prev_hunk(),

            KeyCode::Char('g') => self.scroll = 0,
            KeyCode::Char('G') => self.scroll = self.max_scroll(),

            _ => {}
        }
    }

    /// Key handling for the Files tab (repo browser + content viewer).
    fn handle_files_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Char('/') => self.open_repo_search(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Tree => Focus::Diff,
                    Focus::Diff => Focus::Tree,
                }
            }
            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                Focus::Tree => self.browser.cursor_down(),
                Focus::Diff => self.browser.scroll_content_down(1),
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                Focus::Tree => self.browser.cursor_up(),
                Focus::Diff => self.browser.scroll_content_up(1),
            },
            KeyCode::Char('d') if ctrl => self.browser.scroll_content_down(10),
            KeyCode::Char('u') if ctrl => self.browser.scroll_content_up(10),
            KeyCode::Enter => {
                if self.browser.cursor_is_dir() {
                    self.browser.toggle();
                } else {
                    self.focus = Focus::Diff; // focus the content pane
                }
            }
            KeyCode::Right => self.browser.expand_or_open(),
            KeyCode::Left => self.browser.collapse(),
            _ => {}
        }
    }
}

/// Open the repository at the current directory, resolve the comparison, and run.
pub fn run(config: Config, inv: crate::cli::Invocation) -> Result<()> {
    use crate::git::git2_backend::Git2Backend;

    let cwd = std::env::current_dir().context("get current directory")?;
    let backend = Git2Backend::open(&cwd)?;
    let smart = inv.smart || config.smart_compare;
    let theme_choice = config.theme;
    let resolved = base::resolve(&backend, inv.spec, smart, &config.base_branches)?;

    let mut app = App::with_title(config, Box::new(backend), resolved.spec, resolved.title)?;

    // Resolve the theme mode — detection must happen before raw mode / alt-screen.
    let mode = match theme_choice {
        crate::config::ThemeChoice::Auto => crate::highlight::detect_mode(),
        crate::config::ThemeChoice::Light => ThemeMode::Light,
        crate::config::ThemeChoice::Dark => ThemeMode::Dark,
    };
    app.set_theme_mode(mode);

    let root = app.context().root.clone();

    // One channel the UI loop blocks on: terminal input + job results + file changes.
    let (tx, rx) = crossbeam_channel::unbounded::<jobs::AppEvent>();
    jobs::spawn_input(tx.clone());

    // Watch the working tree (best-effort), forwarding changes onto the channel.
    let watch = watcher::watch(&root).ok();
    if let Some(w) = &watch {
        let (wtx, wrx) = (tx.clone(), w.rx.clone());
        std::thread::spawn(move || {
            while wrx.recv().is_ok() {
                if wtx.send(jobs::AppEvent::FileChanged).is_err() {
                    break;
                }
            }
        });
    }

    let mut terminal = ratatui::try_init()?;
    let result = run_loop(&mut app, &mut terminal, &rx, &tx, &root);
    ratatui::restore();
    drop(watch); // stop watching
    result
}

fn run_loop(
    app: &mut App,
    terminal: &mut DefaultTerminal,
    rx: &crossbeam_channel::Receiver<jobs::AppEvent>,
    tx: &crossbeam_channel::Sender<jobs::AppEvent>,
    root: &std::path::Path,
) -> Result<()> {
    // Event-driven: redraw only when something changed; heavy work runs on
    // background threads and posts results as events.
    let mut dirty = true;
    while !app.should_quit() {
        if dirty {
            terminal.draw(|f| app.render(f))?;
            dirty = false;
        }

        // Spawn any queued async refresh, then redraw the loading state. Read
        // the context fresh each time so `o` (expand context) takes effect.
        if let Some((epoch, spec)) = app.take_pending_refresh() {
            let context = app.config().context_lines;
            jobs::spawn_diff(tx.clone(), root.to_path_buf(), spec, context, epoch);
            dirty = true;
            continue;
        }

        // Spawn any queued async repo-wide search (Files tab).
        if let Some((epoch, query, mode)) = app.take_pending_search() {
            jobs::spawn_search(tx.clone(), root.to_path_buf(), query, mode, epoch);
            dirty = true;
            continue;
        }

        match rx.recv() {
            Ok(jobs::AppEvent::Term(Event::Key(key))) => {
                app.handle_key(key);
                dirty = true;
            }
            Ok(jobs::AppEvent::Term(Event::Resize(_, _))) => dirty = true,
            Ok(jobs::AppEvent::Term(_)) => {}
            Ok(jobs::AppEvent::FileChanged) => {
                if app.auto_refresh() && app.spec_is_live() {
                    app.request_refresh();
                }
            }
            Ok(jobs::AppEvent::DiffReady { epoch, files }) => {
                app.apply_diff_result(epoch, files);
                dirty = true;
            }
            Ok(jobs::AppEvent::SearchReady { epoch, results }) => {
                app.apply_search_result(epoch, results);
                dirty = true;
            }
            // The input thread holds a sender for the whole run, so recv only
            // errors at shutdown; the loop normally exits via `should_quit`.
            // Detached worker/input threads are abandoned when main returns.
            Err(_) => break,
        }

        // Honor a queued editor request: suspend the TUI, run the editor, resume.
        if let Some((path, line)) = app.take_editor_request() {
            launch_editor(terminal, &path, line)?;
            dirty = true;
        }
    }
    Ok(())
}

/// Suspend the TUI, open `$EDITOR`/`$VISUAL` at `path:line`, then restore.
fn launch_editor(terminal: &mut DefaultTerminal, path: &std::path::Path, line: u32) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    ratatui::restore();
    let mut cmd = std::process::Command::new(&editor);
    if editor.ends_with("code") || editor.ends_with("codium") {
        cmd.arg("-g").arg(format!("{}:{line}", path.display()));
    } else {
        cmd.arg(format!("+{line}")).arg(path);
    }
    let _ = cmd.status(); // editor exit status is not actionable
    *terminal = ratatui::try_init()?;
    terminal.clear()?;
    Ok(())
}
