//! Renders the Files tab: a repo tree on the left and the selected file's
//! syntax-highlighted content on the right.

use crate::app::{App, Focus};
use crate::browser::EntryKind;
use crate::highlight::FgSpan;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

const TREE_WIDTH: u16 = 36;

pub fn render(app: &App, f: &mut Frame, area: Rect) {
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

    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(scroll).take(height) {
        let selected = i == cursor;
        let row_style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        let indent = "  ".repeat(row.depth);
        let text = match &row.kind {
            EntryKind::Dir { expanded } => {
                let arrow = if *expanded { '▾' } else { '▸' };
                format!("{indent}{arrow} {}/", row.name)
            }
            EntryKind::File => format!("{indent}  {}", row.name),
        };
        let style = match &row.kind {
            EntryKind::Dir { .. } if !selected => row_style.fg(Color::Blue),
            _ => row_style,
        };
        let mut spans = vec![Span::styled(text.clone(), style)];
        let used = text.chars().count();
        if used < width {
            spans.push(Span::styled(" ".repeat(width - used), row_style));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// Plain syntax-highlighted spans for one line (foreground only, no diff bg).
fn syntax_spans(text: &str, fg: &[FgSpan]) -> Vec<Span<'static>> {
    if fg.is_empty() {
        return vec![Span::raw(text.to_string())];
    }
    fg.iter()
        .filter(|s| {
            s.start < s.end
                && s.end <= text.len()
                && text.is_char_boundary(s.start)
                && text.is_char_boundary(s.end)
        })
        .map(|s| {
            Span::styled(
                text[s.start..s.end].to_string(),
                Style::default().fg(s.color),
            )
        })
        .collect()
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
    app.browser().set_content_viewport(inner.height as usize);

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
    let scroll = app
        .browser()
        .content_scroll()
        .min(total.saturating_sub(height));
    let gutter_w = total.to_string().len().max(2);

    let mut lines: Vec<Line> = Vec::new();
    for (idx, text) in loaded.lines.iter().enumerate().skip(scroll).take(height) {
        let mut spans = vec![Span::styled(
            format!("{:>gutter_w$} ", idx + 1),
            Style::default().fg(Color::DarkGray),
        )];
        // Per-line spans precomputed with carried state (multi-line aware, M12).
        let empty = Vec::new();
        let fg = loaded.highlights.get(idx).unwrap_or(&empty);
        spans.extend(syntax_spans(text, fg));
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), content);
    super::render_scrollbar(f, inner, total, scroll);
}
