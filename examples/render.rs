//! Render a single gdiff frame to stdout as text, for headless inspection.
//!
//! `cargo run --example render` — the agent's "eyes" on the UI without a terminal.
//! As the UI grows, accept a scenario name and build a matching fixture + state.

use anyhow::Result;
use gdiff::app::App;
use gdiff::config::Config;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn main() -> Result<()> {
    let app = App::new(Config::default());

    let mut terminal = Terminal::new(TestBackend::new(100, 24))?;
    terminal.draw(|f| app.render(f))?;

    // TestBackend's Display renders the buffer as quoted text rows.
    println!("{}", terminal.backend());
    Ok(())
}
