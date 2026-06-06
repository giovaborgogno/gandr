//! Frame layout and rendering.
//!
//! Render functions take immutable [`App`] state and draw into a ratatui `Frame`
//! with no I/O, so they're snapshot-testable on `TestBackend` (see docs/testing.md).

pub mod viewer_unified;

use crate::app::{App, Focus};
use crate::diff::FileDiff;
use crate::highlight::{Highlighter, Palette};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// Width of the file list panel, in columns.
const TREE_WIDTH: u16 = 30;

/// Border highlight when a pane is focused.
fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Draw the whole frame.
pub fn render(app: &App, f: &mut Frame) {
    let [header, body, keybar] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(
        Paragraph::new(Span::styled(
            app.header_line(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        header,
    );

    let [tree, viewer] =
        Layout::horizontal([Constraint::Length(TREE_WIDTH), Constraint::Min(0)]).areas(body);

    render_file_list(app, f, tree);
    render_viewer(app, f, viewer);

    f.render_widget(
        Paragraph::new(Span::styled(
            app.keybar_line(),
            Style::default().fg(Color::DarkGray),
        )),
        keybar,
    );
}

/// The left panel: a flat list of changed files (the compact tree lands in M4).
fn render_file_list(app: &App, f: &mut Frame, area: Rect) {
    let block = Block::bordered()
        .title(format!("Files ({})", app.files().len()))
        .border_style(border_style(app.focus() == Focus::Tree));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    for (i, file) in app.files().iter().enumerate() {
        let selected = i == app.selected();
        let marker = file.change.status.marker();
        let path = file.change.path.to_string_lossy();
        let text = format!("{marker} {path}");

        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(text, style)));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The right panel: a sticky file header plus the scrollable diff body.
fn render_viewer(app: &App, f: &mut Frame, area: Rect) {
    let block = Block::bordered().border_style(border_style(app.focus() == Focus::Diff));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(file) = app.current() else {
        f.render_widget(
            Paragraph::new(
                "No uncommitted changes. Press `c` to compare against a branch, or run with --smart.",
            ),
            inner,
        );
        app.set_viewport(0);
        return;
    };

    let [file_header, diff_body] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);

    f.render_widget(file_header_line(app, file), file_header);
    app.set_viewport(diff_body.height as usize);

    let hl = Highlighter::for_path(&file.change.path);
    let palette = Palette::default();
    viewer_unified::render(
        f,
        diff_body,
        file,
        app.scroll(),
        &hl,
        &palette,
        app.config().word_diff,
    );
}

/// The sticky one-line file header: path, per-file counts, and position.
fn file_header_line<'a>(app: &App, file: &'a FileDiff) -> Paragraph<'a> {
    let path = file.change.path.to_string_lossy().into_owned();
    let pos = format!("[{}/{}]", app.selected() + 1, app.files().len());
    Paragraph::new(Line::from(vec![
        Span::styled(path, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(
            format!("+{} −{}", file.change.additions, file.change.deletions),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(pos, Style::default().fg(Color::DarkGray)),
    ]))
}
