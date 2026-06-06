//! Frame layout and rendering.
//!
//! Render functions take immutable [`App`] state and draw into a ratatui `Frame`
//! with no I/O, so they're snapshot-testable on `TestBackend` (see docs/testing.md).

pub mod tree;
pub mod viewer_split;
pub mod viewer_unified;

use crate::app::{App, Focus};
use crate::config::ViewMode;
use crate::diff::FileDiff;
use crate::highlight::{Highlighter, Palette};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// Width of the file tree panel, in columns.
const TREE_WIDTH: u16 = 32;

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

    let [tree_area, viewer] =
        Layout::horizontal([Constraint::Length(TREE_WIDTH), Constraint::Min(0)]).areas(body);

    render_file_list(app, f, tree_area);
    render_viewer(app, f, viewer);

    // The keybar doubles as the search prompt while searching.
    let (keybar_text, keybar_style) = match app.search() {
        Some(s) if s.editing => (format!("/{}", s.query), Style::default().fg(Color::Yellow)),
        Some(s) => (
            format!("/{}   n/N next·prev · Esc close", s.query),
            Style::default().fg(Color::Yellow),
        ),
        None => (app.keybar_line(), Style::default().fg(Color::DarkGray)),
    };
    f.render_widget(
        Paragraph::new(Span::styled(keybar_text, keybar_style)),
        keybar,
    );

    if let Some(picker) = app.picker() {
        render_picker(f, f.area(), picker);
    }
    if app.show_help() {
        render_help(f, f.area());
    }
}

/// Draw the help overlay listing keybindings.
fn render_help(f: &mut Frame, area: Rect) {
    const KEYS: &[(&str, &str)] = &[
        ("j / k, ↑ / ↓", "move (tree) or scroll (diff)"),
        ("Tab", "switch focus tree ↔ diff"),
        ("n / p", "next / previous file"),
        ("] / [", "next / previous hunk"),
        (
            "Enter, → / ←",
            "expand / collapse tree; Enter on a file focuses diff",
        ),
        ("g / G, Ctrl-d/u", "top / bottom; half-page"),
        ("s", "toggle unified / side-by-side"),
        ("w", "toggle word-level highlight"),
        ("Space", "mark file reviewed"),
        ("c", "compare picker"),
        ("/", "search in diff (n/N to navigate)"),
        ("e", "open file in $EDITOR"),
        ("r / a", "refresh / toggle auto-refresh"),
        ("? / q", "this help / quit"),
    ];

    // Clamp to the area so the popup always fits (no oversized Rect on tiny terminals).
    let width = 60.min(area.width);
    let height = (KEYS.len() as u16 + 2).min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect {
        x,
        y,
        width,
        height,
    };

    let block = Block::bordered()
        .title("Keys")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(block, popup);

    let lines: Vec<Line> = KEYS
        .iter()
        .map(|(k, desc)| {
            Line::from(vec![
                Span::styled(format!(" {k:<16}"), Style::default().fg(Color::Yellow)),
                Span::raw(*desc),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// Draw the compare-picker overlay centered over the frame.
fn render_picker(f: &mut Frame, area: Rect, picker: &crate::app::Picker) {
    // Clamp to the area so the popup always fits (no oversized Rect on tiny terminals).
    let width = 40.min(area.width);
    let height = (picker.items.len() as u16 + 2).min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect {
        x,
        y,
        width,
        height,
    };

    let block = Block::bordered()
        .title("Compare against…")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(block, popup);

    let lines: Vec<Line> = picker
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == picker.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Line::from(Span::styled(format!(" {} ", item.label), style))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// The left panel: the compact file tree.
fn render_file_list(app: &App, f: &mut Frame, area: Rect) {
    let block = Block::bordered()
        .title(format!("Files ({})", app.files().len()))
        .border_style(border_style(app.focus() == Focus::Tree));

    let inner = block.inner(area);
    let rows = app.tree_rows();
    let scroll = app.tree_scroll(inner.height as usize);
    tree::render(
        f,
        area,
        app.files(),
        &rows,
        app.review_statuses(),
        app.tree_cursor(),
        scroll,
        block,
    );
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

    let mode = app.theme_mode();
    let hl = Highlighter::for_path(&file.change.path, mode);
    let palette = Palette::for_mode(mode);
    let word_on = app.config().word_diff;
    match app.view() {
        ViewMode::Unified => {
            viewer_unified::render(f, diff_body, file, app.scroll(), &hl, &palette, word_on)
        }
        ViewMode::SideBySide => {
            viewer_split::render(f, diff_body, file, app.scroll(), &hl, &palette, word_on)
        }
    }
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
