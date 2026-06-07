//! Renders the Files tab: a repo tree on the left and the selected file's
//! syntax-highlighted content on the right.

use crate::app::{App, Focus};
use crate::browser::EntryKind;
use crate::git::Status;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

const TREE_WIDTH: u16 = 36;

/// Tree color for a file's change status (matches a git-client palette).
fn status_color(s: Status) -> Color {
    match s {
        Status::Added => Color::Green,
        Status::Modified => Color::Yellow,
        Status::Deleted => Color::Red,
        Status::Renamed | Status::Copied => Color::Cyan,
    }
}

pub fn render(app: &App, f: &mut Frame, area: Rect) {
    if !app.show_tree() {
        render_content(app, f, area); // tree hidden → full-width content
        return;
    }
    let [tree_area, content_area] =
        Layout::horizontal([Constraint::Length(TREE_WIDTH), Constraint::Min(0)]).areas(area);
    render_tree(app, f, tree_area);
    render_content(app, f, content_area);
}

fn render_tree(app: &App, f: &mut Frame, area: Rect) {
    let browser = app.browser();
    let block = Block::bordered()
        .title("Repo")
        .border_style(super::border_style(app.focus() == Focus::Tree));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = browser.rows();
    let height = inner.height as usize;
    let scroll = browser.tree_scroll(height);
    let cursor = browser.cursor();
    let width = inner.width as usize;

    // Which files/dirs changed in the current comparison (the same set as the
    // Diff tab) — so the Repo browser shows at a glance what's modified. Files
    // get a colored M/A/D marker; a directory containing changes gets a dot.
    // Derived once per refresh (App::rebuild_repo_status), not rebuilt per frame.
    let root = &app.context().root;
    let changed = app.repo_status();
    let changed_dirs = app.repo_status_dirs();

    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(scroll).take(height) {
        let selected = i == cursor;
        let row_style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        let rel = row.path.strip_prefix(root).unwrap_or(&row.path);
        let file_status = match row.kind {
            EntryKind::File => changed.get(rel).copied(),
            EntryKind::Dir { .. } => None,
        };
        let dir_touched = matches!(row.kind, EntryKind::Dir { .. }) && changed_dirs.contains(rel);

        // 2-col status gutter, then the indented arrow/name.
        let (mark, mark_color) = match (file_status, dir_touched) {
            (Some(st), _) => (st.marker(), status_color(st)),
            (None, true) => ('•', Color::DarkGray),
            _ => (' ', Color::DarkGray),
        };
        let indent = "  ".repeat(row.depth);
        let body = match &row.kind {
            EntryKind::Dir { expanded } => {
                let arrow = if *expanded { '▾' } else { '▸' };
                format!("{indent}{arrow} {}/", row.name)
            }
            EntryKind::File => format!("{indent}  {}", row.name),
        };
        let body_style = if selected {
            row_style
        } else if let Some(st) = file_status {
            row_style.fg(status_color(st))
        } else if matches!(row.kind, EntryKind::Dir { .. }) {
            row_style.fg(Color::Blue)
        } else {
            row_style
        };
        let gutter_style = if selected {
            row_style
        } else {
            Style::default().fg(mark_color)
        };
        let mut spans = vec![
            Span::styled(format!("{mark} "), gutter_style),
            Span::styled(body.clone(), body_style),
        ];
        let used = 2 + body.chars().count();
        if used < width {
            spans.push(Span::styled(" ".repeat(width - used), row_style));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_content(app: &App, f: &mut Frame, area: Rect) {
    // Title the pane with the selected file's path (relative to the repo root).
    let title = app.browser().loaded().map(|l| {
        let p = l.path.strip_prefix(&app.context().root).unwrap_or(&l.path);
        format!(" {} ", p.display())
    });
    let mut block = Block::bordered().border_style(super::border_style(app.focus() == Focus::Diff));
    if let Some(title) = &title {
        block = block.title(title.clone());
    }
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(loaded) = app.browser().loaded() else {
        f.render_widget(Paragraph::new("Select a file to view its contents."), inner);
        return;
    };
    if loaded.too_large {
        f.render_widget(Paragraph::new("File too large to preview."), inner);
        return;
    }
    if loaded.binary {
        f.render_widget(Paragraph::new("Binary file."), inner);
        return;
    }

    let content = Rect {
        width: inner.width.saturating_sub(1),
        ..inner
    };
    let height = inner.height as usize;
    let total = loaded.lines.len();
    let scroll = app.browser().content_scroll(height);
    let cursor = app.browser().content_cursor();
    let focused = app.focus() == Focus::Diff; // content pane focused
    let gutter_w = total.to_string().len().max(2);
    let cw = content.width as usize;
    // Highlight content-search matches in the preview (after a repo content jump).
    let query = app.browser_query().filter(|q| !q.is_empty());
    let palette = crate::highlight::Palette::for_mode(app.theme_mode());
    // Visual selection (v/y to copy) — selected rows get a selection background.
    let selection = app.browser().content_selection();
    let sel_bg = Color::Rgb(45, 55, 78);

    let mut lines: Vec<Line> = Vec::new();
    for (idx, text) in loaded.lines.iter().enumerate().skip(scroll).take(height) {
        let selected = selection.is_some_and(|(lo, hi)| idx >= lo && idx <= hi);
        // The current line's number is reversed ("you are here").
        let gutter_style = if focused && idx == cursor {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let mut spans = vec![Span::styled(
            format!("{:>gutter_w$} ", idx + 1),
            gutter_style,
        )];
        // Syntax spans arrive from a background job (`highlight_target`); until
        // then `fg` is empty and the line renders plain — so selecting a file
        // never blocks on syntect. The composer still applies the search match.
        let empty = Vec::new();
        let fg = loaded.highlights.get(idx).unwrap_or(&empty);
        spans.extend(crate::highlight::compose::line_spans(
            text,
            crate::diff::LineKind::Context,
            &[],
            fg,
            &palette,
            false,
            query,
        ));
        if selected {
            // Pad to the panel edge so the selection background spans the row.
            let used = gutter_w + 1 + text.chars().count();
            if used < cw {
                spans.push(Span::raw(" ".repeat(cw - used)));
            }
            lines.push(Line::from(spans).style(Style::default().bg(sel_bg)));
        } else {
            lines.push(Line::from(spans));
        }
    }
    f.render_widget(Paragraph::new(lines), content);
    super::render_scrollbar(f, inner, total, scroll);
}
