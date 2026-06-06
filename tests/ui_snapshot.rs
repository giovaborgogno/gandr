//! Snapshot tests: drive the real `App`, render to `TestBackend`, golden the frame.
//! This is gdiff's deterministic headless "e2e" (see docs/testing.md).

use gdiff::app::App;
use gdiff::config::Config;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Render the App to a text frame of the given size.
fn frame(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().to_string()
}

#[test]
fn empty_app_renders_skeleton() {
    let app = App::new(Config::default());
    insta::assert_snapshot!(frame(&app, 80, 16));
}
