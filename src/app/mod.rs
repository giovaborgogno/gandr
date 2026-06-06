//! Application state, the event loop, and key dispatch.

use crate::config::{Config, ViewMode};
use crate::diff::{engine, FileDiff};
use crate::git::{base, CompareSpec, GitBackend, RepoContext};
use crate::ui;
use crate::ui::tree::{self, Row, RowKind};
use anyhow::{Context as _, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};
use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

/// How long the loop blocks waiting for input before redrawing.
const TICK: Duration = Duration::from_millis(250);

/// Which pane the directional keys act on. Navigation is *hybrid*: `Tab` switches
/// focus (driving `j`/`k`), but `n`/`p` (file) and `]`/`[` (hunk) work regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Diff,
}

/// What a compare-picker entry does when chosen.
#[derive(Debug, Clone)]
enum PickerAction {
    Spec(CompareSpec),
    /// Resolve the current branch's PR via `gh` on selection.
    Pr,
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
    /// The compare picker overlay, when open.
    picker: Option<Picker>,
    should_quit: bool,
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
            picker: None,
            should_quit: false,
        };
        app.reset_view();
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

    /// The header line: comparison + branch + file count + totals.
    pub fn header_line(&self) -> String {
        let branch = self.context.branch.as_deref().unwrap_or("(detached)");
        let (add, del) = self.totals();
        let n = self.files.len();
        let files = if n == 1 { "file" } else { "files" };
        let label = self.title.clone().unwrap_or_else(|| self.spec.label());
        format!("gdiff · {branch} · {label} · {n} {files}  +{add} −{del}")
    }

    /// The contextual key hints at the bottom (or the last error, if any).
    pub fn keybar_line(&self) -> String {
        if let Some(err) = &self.error {
            return format!("⚠ {err}");
        }
        "j/k move · Tab focus · n/p file · ]/[ hunk · c compare · s split · w word · q quit"
            .to_string()
    }

    /// Let the renderer record the diff-viewport height for page math.
    pub fn set_viewport(&self, rows: usize) {
        self.viewport.set(rows);
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

    // ---- diff-row geometry (kept in sync with viewer_unified row layout) ----

    /// Row offset of each hunk header within the selected file's viewer.
    fn hunk_offsets(&self) -> Vec<usize> {
        let Some(file) = self.current() else {
            return Vec::new();
        };
        let mut offsets = Vec::with_capacity(file.hunks.len());
        let mut row = 0;
        for hunk in &file.hunks {
            offsets.push(row);
            row += 1 + hunk.lines.len(); // 1 header row + its lines
        }
        offsets
    }

    /// Total viewer rows for the selected file.
    fn total_rows(&self) -> usize {
        self.current()
            .map(|f| f.hunks.iter().map(|h| 1 + h.lines.len()).sum())
            .unwrap_or(0)
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

    /// Switch the comparison and recompute the diff, reverting on failure.
    fn set_spec(&mut self, spec: CompareSpec, title: Option<String>) {
        match engine::build_diffs(self.backend.as_ref(), &spec, self.config.context_lines) {
            Ok(files) => {
                self.spec = spec;
                self.title = title;
                self.error = None;
                self.files = files;
                self.collapsed.clear();
                self.reset_view();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// The open compare picker, if any (for rendering).
    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
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

        // Ctrl-C always quits (even with the picker open), before `c` opens it.
        if ctrl && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        // The compare picker, when open, captures all other keys.
        if self.picker_key(key) {
            return;
        }
        let half_page = (self.viewport.get() / 2).max(1);

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') => self.open_picker(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Tree => Focus::Diff,
                    Focus::Diff => Focus::Tree,
                }
            }
            KeyCode::Char('s') => self.view = self.view.toggled(),
            KeyCode::Char('w') => self.config.word_diff = !self.config.word_diff,

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
}

/// Open the repository at the current directory, resolve the comparison, and run.
pub fn run(config: Config, inv: crate::cli::Invocation) -> Result<()> {
    use crate::git::git2_backend::Git2Backend;

    let cwd = std::env::current_dir().context("get current directory")?;
    let backend = Git2Backend::open(&cwd)?;
    let smart = inv.smart || config.smart_compare;
    let resolved = base::resolve(&backend, inv.spec, smart, &config.base_branches)?;

    let mut app = App::with_title(config, Box::new(backend), resolved.spec, resolved.title)?;
    let mut terminal = ratatui::try_init()?;
    let result = run_loop(&mut app, &mut terminal);
    ratatui::restore();
    result
}

fn run_loop(app: &mut App, terminal: &mut DefaultTerminal) -> Result<()> {
    while !app.should_quit() {
        terminal.draw(|f| app.render(f))?;
        if event::poll(TICK)? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }
    }
    Ok(())
}
