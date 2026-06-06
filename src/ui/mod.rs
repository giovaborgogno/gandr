//! Frame layout and rendering.
//!
//! Render functions take immutable [`App`] state and draw into a ratatui `Frame`
//! with no I/O, so they're snapshot-testable on `TestBackend` (see docs/testing.md).
//! M0 draws the empty skeleton: header / files panel / viewer / keybar.

use crate::app::App;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// Width of the file tree panel, in columns.
const TREE_WIDTH: u16 = 28;

/// Draw the whole frame.
pub fn render(app: &App, f: &mut Frame) {
    let [header, body, keybar] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(Paragraph::new(app.header_line()), header);

    let [tree, viewer] =
        Layout::horizontal([Constraint::Length(TREE_WIDTH), Constraint::Min(0)]).areas(body);

    f.render_widget(
        Block::bordered().title(format!("Files ({})", app.file_count())),
        tree,
    );
    f.render_widget(
        Paragraph::new(app.viewer_placeholder()).block(Block::bordered()),
        viewer,
    );

    f.render_widget(Paragraph::new(app.keybar_line()), keybar);
}
