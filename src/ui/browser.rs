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

use super::TREE_WIDTH;

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
        let rel = row.path.strip_prefix(root).unwrap_or(&row.path);
        let file_status = match row.kind {
            EntryKind::File => changed.get(rel).copied(),
            EntryKind::Dir { .. } => None,
        };
        let dir_touched = matches!(row.kind, EntryKind::Dir { .. }) && changed_dirs.contains(rel);

        // Files carry their M/A/D/R marker inline before the name. A directory
        // that contains changes has its name tinted yellow (no marker column),
        // so you can spot modified subtrees even while they're collapsed.
        let marker = file_status.map(|st| (st.marker(), status_color(st)));
        let (arrow, label_color) = match &row.kind {
            EntryKind::Dir { expanded } => {
                let color = if dir_touched {
                    Color::Yellow
                } else {
                    Color::Blue
                };
                (Some(*expanded), Some(color))
            }
            EntryKind::File => (None, file_status.map(status_color)),
        };
        lines.push(super::tree_row_line(
            width,
            selected,
            row.depth,
            None, // the Repo tab has no review state — no review column
            marker,
            arrow,
            &row.name,
            label_color,
        ));
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
        if app.render_image(f, inner) {
            return;
        }
        let text = match &loaded.image {
            Some(info) => format!("Image · {}", info.summary()),
            None => "Binary file.".to_string(),
        };
        f.render_widget(Paragraph::new(text), inner);
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
    let sel_bg = super::viewer_unified::SELECTION_BG;

    // Long lines wrap within the content column (matching the diff viewer)
    // rather than truncating. Scrolling is per logical line: each line starts a
    // fresh row, and its wrapped continuations carry a blank gutter.
    let wrap_w = cw.saturating_sub(gutter_w + 1).max(1);
    // How many terminal rows a logical line occupies once wrapped to `wrap_w`.
    // `wrap_spans` breaks on every `wrap_w`th char, so this matches its output.
    let disp_rows = |i: usize| -> usize {
        let n = loaded.lines.get(i).map_or(0, |t| t.chars().count());
        n.div_ceil(wrap_w).max(1)
    };
    // Because wrapped lines take several terminal rows, the logical-line scroll
    // from `content_scroll` isn't enough to keep the cursor on screen. Mirror the
    // unified viewer: start at the follow-scroll line, then pull the top down
    // until the cursor's wrapped rows fit in `height` (so the cursor is always
    // visible without measuring the whole file).
    let mut top = scroll.min(total.saturating_sub(1)).min(cursor);
    while top < cursor {
        let used: usize = (top..=cursor).map(disp_rows).sum();
        if used <= height {
            break;
        }
        top += 1;
    }

    let mut lines: Vec<Line> = Vec::new();
    for (idx, text) in loaded.lines.iter().enumerate().skip(top) {
        if lines.len() >= height {
            break;
        }
        let selected = selection.is_some_and(|(lo, hi)| idx >= lo && idx <= hi);
        // The current line's number is reversed ("you are here").
        let gutter_style = if focused && idx == cursor {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        // Syntax spans arrive from a background job (`highlight_target`); until
        // then `fg` is empty and the line renders plain — so selecting a file
        // never blocks on syntect. The composer still applies the search match.
        let empty = Vec::new();
        let fg = loaded.highlights.get(idx).unwrap_or(&empty);
        let content_spans = crate::highlight::compose::line_spans(
            text,
            crate::diff::LineKind::Context,
            &[],
            fg,
            &palette,
            false,
            query,
        );
        let wrapped = crate::ui::viewer_split::wrap_spans(&content_spans, wrap_w);
        for (sub, seg) in wrapped.into_iter().enumerate() {
            if lines.len() >= height {
                break;
            }
            let gutter = if sub == 0 {
                Span::styled(format!("{:>gutter_w$} ", idx + 1), gutter_style)
            } else {
                Span::styled(" ".repeat(gutter_w + 1), Style::default())
            };
            let mut spans = vec![gutter];
            let seg_w: usize = seg.iter().map(|s| s.content.chars().count()).sum();
            spans.extend(seg);
            if selected {
                // Pad to the panel edge so the selection background spans the row.
                let used = gutter_w + 1 + seg_w;
                if used < cw {
                    spans.push(Span::raw(" ".repeat(cw - used)));
                }
                lines.push(Line::from(spans).style(Style::default().bg(sel_bg)));
            } else {
                lines.push(Line::from(spans));
            }
        }
    }
    f.render_widget(Paragraph::new(lines), content);
    // Scrollbar is approximated in logical lines (matching the unified viewer);
    // `top` is the first logical line drawn after the cursor-visibility pass.
    super::render_scrollbar(f, inner, total, top);
}
