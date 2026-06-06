//! Application state, the event loop, and key dispatch.

use crate::config::{Config, ViewMode};
use crate::ui;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{DefaultTerminal, Frame};
use std::time::Duration;

/// How long the loop blocks waiting for input before redrawing.
const TICK: Duration = Duration::from_millis(250);

/// The whole application state. Diff/tree/scroll state arrives in later milestones.
pub struct App {
    pub config: Config,
    /// Current view layout (toggled with `s`).
    pub view: ViewMode,
    should_quit: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        let view = config.default_view;
        Self {
            config,
            view,
            should_quit: false,
        }
    }

    /// The top header line (comparison + stats). Filled in as state grows.
    pub fn header_line(&self) -> String {
        "gdiff".to_string()
    }

    /// Number of changed files (none until the git layer lands in M1).
    pub fn file_count(&self) -> usize {
        0
    }

    /// Placeholder shown in the viewer when there's nothing to diff yet.
    pub fn viewer_placeholder(&self) -> String {
        "No uncommitted changes. Press `c` to compare against a branch, or run with --smart."
            .to_string()
    }

    /// The contextual key hints at the bottom.
    pub fn keybar_line(&self) -> String {
        "q quit · s split · ? help".to_string()
    }

    /// Whether the loop should exit.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Render the current state into a frame.
    pub fn render(&self, f: &mut Frame) {
        ui::render(self, f);
    }

    /// Handle a single key event. Pure state transition — easy to drive in tests.
    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('s') => self.view = self.view.toggled(),
            _ => {}
        }
    }
}

/// Build the app from config and run it on the real terminal.
pub fn run(config: Config) -> Result<()> {
    let mut app = App::new(config);
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
