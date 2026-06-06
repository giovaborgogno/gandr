//! Application state, the event loop, and key dispatch.

use crate::config::{Config, ViewMode};
use crate::diff::FileDiff;
use crate::git::{CompareSpec, RepoContext};
use crate::ui;
use anyhow::{Context as _, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};
use std::cell::Cell;
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

/// The whole application state.
pub struct App {
    config: Config,
    view: ViewMode,
    focus: Focus,
    context: RepoContext,
    spec: CompareSpec,
    files: Vec<FileDiff>,
    /// Index into `files` of the selected file.
    selected: usize,
    /// First visible row of the diff viewer for the selected file.
    scroll: usize,
    /// Diff-viewport height from the last render, for page/bottom math.
    viewport: Cell<usize>,
    should_quit: bool,
}

impl App {
    pub fn new(
        config: Config,
        context: RepoContext,
        spec: CompareSpec,
        files: Vec<FileDiff>,
    ) -> Self {
        let view = config.default_view;
        Self {
            config,
            view,
            focus: Focus::Tree,
            context,
            spec,
            files,
            selected: 0,
            scroll: 0,
            viewport: Cell::new(0),
            should_quit: false,
        }
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
        format!(
            "gdiff · {branch} · {} · {n} {files}  +{add} −{del}",
            self.spec.label()
        )
    }

    /// The contextual key hints at the bottom.
    pub fn keybar_line(&self) -> String {
        "j/k move · Tab focus · n/p file · ]/[ hunk · s split · q quit · ? help".to_string()
    }

    /// Let the renderer record the diff-viewport height for page math.
    pub fn set_viewport(&self, rows: usize) {
        self.viewport.set(rows);
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

    fn select(&mut self, idx: usize) {
        if self.files.is_empty() {
            return;
        }
        self.selected = idx.min(self.files.len() - 1);
        self.scroll = 0;
    }

    fn select_next(&mut self) {
        self.select(self.selected + 1);
    }

    fn select_prev(&mut self) {
        self.select(self.selected.saturating_sub(1));
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

    // ---- rendering & input ----

    pub fn render(&self, f: &mut Frame) {
        ui::render(self, f);
    }

    /// Handle a single key event. Pure state transition — easy to drive in tests.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let half_page = (self.viewport.get() / 2).max(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
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
                Focus::Tree => self.select_next(),
                Focus::Diff => self.scroll_down(1),
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                Focus::Tree => self.select_prev(),
                Focus::Diff => self.scroll_up(1),
            },

            KeyCode::Char('n') => self.select_next(),
            KeyCode::Char('p') => self.select_prev(),
            KeyCode::Char(']') => self.next_hunk(),
            KeyCode::Char('[') => self.prev_hunk(),

            KeyCode::Char('g') => self.scroll = 0,
            KeyCode::Char('G') => self.scroll = self.max_scroll(),

            _ => {}
        }
    }
}

/// Open the repository at the current directory, compute the diff, and run the TUI.
pub fn run(config: Config, spec: CompareSpec) -> Result<()> {
    use crate::diff::engine;
    use crate::git::git2_backend::Git2Backend;
    use crate::git::GitBackend;

    let cwd = std::env::current_dir().context("get current directory")?;
    let backend = Git2Backend::open(&cwd)?;
    let context = backend.context()?;
    let files = engine::build_diffs(&backend, &spec, config.context_lines)?;

    let mut app = App::new(config, context, spec, files);
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
