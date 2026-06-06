//! Frame layout and rendering.
//!
//! Render functions take immutable [`App`] state and draw into a ratatui `Frame`
//! with no I/O, so they're snapshot-testable on `TestBackend` (see docs/testing.md).

pub mod browser;
pub mod tree;
pub mod viewer_split;
pub mod viewer_unified;

use crate::app::{App, Focus, Tab};
use crate::config::ViewMode;
use crate::diff::FileDiff;
use crate::highlight::Palette;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

/// Width of the file tree panel, in columns.
const TREE_WIDTH: u16 = 32;

/// Key hints shown in the keybar (key, label).
const KEYBAR: &[(&str, &str)] = &[
    ("j/k", "move"),
    ("Tab", "focus"),
    ("n/p", "file"),
    ("]/[", "hunk"),
    ("c", "compare"),
    ("b", "ref"),
    ("o", "context"),
    ("Space", "review"),
    ("s", "split"),
    ("/", "find"),
    ("e", "edit"),
    ("?", "help"),
    ("q", "quit"),
];

/// Key hints for the Files tab.
const KEYBAR_FILES: &[(&str, &str)] = &[
    ("j/k", "move"),
    ("Tab", "focus"),
    ("Enter", "open/expand"),
    ("←/→", "collapse/expand"),
    ("/", "search repo"),
    ("1", "diff"),
    ("?", "help"),
    ("q", "quit"),
];

/// Build the styled keybar line: keys highlighted, labels dim.
fn keybar_hints(items: &'static [(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = Vec::with_capacity(items.len() * 3);
    for (i, (key, label)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default()));
        }
        spans.push(Span::styled(
            *key,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(spans)
}

/// Draw a vertical scrollbar on the right edge of `area` for `total` rows at `pos`.
/// No-op when everything fits.
pub(crate) fn render_scrollbar(f: &mut Frame, area: Rect, total: usize, pos: usize) {
    let height = area.height as usize;
    if total <= height {
        return;
    }
    let mut state = ScrollbarState::new(total)
        .viewport_content_length(height)
        .position(pos.min(total.saturating_sub(1)));
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .thumb_symbol("█")
        .track_symbol(Some("│"))
        .style(Style::default().fg(Color::DarkGray));
    f.render_stateful_widget(bar, area, &mut state);
}

/// Border highlight when a pane is focused.
fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Draw the whole frame: gitui-style header (tabs + separator), body, keybar.
pub fn render(app: &App, f: &mut Frame) {
    let [tabs, separator, body, keybar] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());

    render_header(app, f, tabs);
    f.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(separator.width as usize),
            Style::default().fg(Color::DarkGray),
        )),
        separator,
    );

    match app.tab() {
        Tab::Diff => {
            let [tree_area, viewer] =
                Layout::horizontal([Constraint::Length(TREE_WIDTH), Constraint::Min(0)])
                    .areas(body);
            render_file_list(app, f, tree_area);
            render_viewer(app, f, viewer);
        }
        Tab::Files => browser::render(app, f, body),
    }

    f.render_widget(Paragraph::new(keybar_line(app)), keybar);

    if let Some(picker) = app.picker() {
        render_picker(f, f.area(), picker);
    }
    if let Some(rp) = app.ref_picker() {
        render_ref_picker(f, f.area(), rp);
    }
    if let Some(rs) = app.repo_search() {
        render_repo_search(f, f.area(), rs);
    }
    if app.show_help() {
        render_help(f, f.area());
    }
}

/// Draw the repo-wide search overlay (Files tab): a query line over a scrollable,
/// selectable results list (file names or content hits).
fn render_repo_search(f: &mut Frame, area: Rect, rs: &crate::app::RepoSearch) {
    use crate::search::SearchResults;

    let width = area.width.saturating_sub(4).clamp(1, 100);
    let height = area.height.saturating_sub(2).clamp(3, 24);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let popup = Rect {
        x,
        y,
        width,
        height,
    };

    let loading = if rs.loading { " · …" } else { "" };
    let title = format!(
        "Search [{}] · {} results{} · Tab: switch mode",
        rs.mode.label(),
        rs.results.len(),
        loading
    );
    let block = Block::bordered()
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(block, popup);

    let [query_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::DarkGray)),
            Span::styled(rs.query.clone(), Style::default().fg(Color::Yellow)),
        ])),
        query_area,
    );

    let rows = list_area.height as usize;
    let cols = list_area.width as usize;
    // Keep the selected result on-screen.
    let scroll = rs.selected.saturating_sub(rows.saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    match &rs.results {
        SearchResults::Files(v) => {
            for (i, p) in v.iter().enumerate().skip(scroll).take(rows) {
                lines.push(result_line(
                    i == rs.selected,
                    p.to_string_lossy().into_owned(),
                    cols,
                ));
            }
        }
        SearchResults::Content(v) => {
            for (i, m) in v.iter().enumerate().skip(scroll).take(rows) {
                let text = format!("{}:{}  {}", m.path.display(), m.line, m.text.trim_start());
                lines.push(result_line(i == rs.selected, text, cols));
            }
        }
    }
    f.render_widget(Paragraph::new(lines), list_area);
}

/// Draw the fuzzy ref picker: a query line over a ranked, selectable list of
/// branches/tags (each tagged with its kind).
fn render_ref_picker(f: &mut Frame, area: Rect, rp: &crate::app::RefPicker) {
    let width = area.width.saturating_sub(4).clamp(1, 70);
    let height = area.height.saturating_sub(2).clamp(3, 20);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let popup = Rect {
        x,
        y,
        width,
        height,
    };

    let title = format!("Compare vs ref · {} matches", rp.filtered.len());
    let block = Block::bordered()
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(ratatui::widgets::Clear, popup);
    f.render_widget(block, popup);

    let [query_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::DarkGray)),
            Span::styled(rp.query.clone(), Style::default().fg(Color::Yellow)),
        ])),
        query_area,
    );

    let rows = list_area.height as usize;
    let cols = list_area.width as usize;
    let scroll = rp.selected.saturating_sub(rows.saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    for (i, &idx) in rp.filtered.iter().enumerate().skip(scroll).take(rows) {
        let entry = &rp.all[idx];
        let selected = i == rp.selected;
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        // "name            (kind)" padded so the kind tag right-aligns-ish.
        let tag = format!(" ({})", entry.kind.label());
        let avail = cols.saturating_sub(tag.chars().count());
        let name: String = entry.name.chars().take(avail).collect();
        let pad = avail.saturating_sub(name.chars().count());
        let mut spans = vec![Span::styled(name, style)];
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), style));
        }
        spans.push(Span::styled(
            tag,
            if selected {
                style
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ));
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), list_area);
}

/// One results-list row: selection-highlighted, truncated, padded to full width.
fn result_line(selected: bool, text: String, width: usize) -> Line<'static> {
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let truncated: String = text.chars().take(width).collect();
    let used = truncated.chars().count();
    let mut spans = vec![Span::styled(truncated, style)];
    if used < width {
        spans.push(Span::styled(" ".repeat(width - used), style));
    }
    Line::from(spans)
}

/// The gitui-style tab bar: `Diff [1]  Files [2]` left, repo path right.
fn render_header(app: &App, f: &mut Frame, area: Rect) {
    // Tabs on the left.
    let mut spans = vec![Span::raw(" ")];
    let mut used = 1usize;
    for (tab, label) in [(Tab::Diff, "Diff"), (Tab::Files, "Files")] {
        let style = if app.tab() == tab {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw("   "));
        used += label.chars().count() + 3;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);

    // Compact context on the right (branch · comparison + live indicators), dim.
    let context = header_context(app);
    let w = area.width as usize;
    if !context.is_empty() && w > used + 1 {
        let avail = w - used - 1;
        let text: String = if context.chars().count() > avail {
            context.chars().take(avail).collect()
        } else {
            context
        };
        let pad = avail - text.chars().count();
        let right = Rect {
            x: area.x + (used + pad) as u16,
            width: (avail - pad + 1) as u16,
            ..area
        };
        f.render_widget(
            Paragraph::new(Span::styled(text, Style::default().fg(Color::DarkGray))),
            right,
        );
    }
}

/// The compact right-aligned header context (counts/reviewed live in the panel
/// titles, so the header stays uncluttered).
fn header_context(app: &App) -> String {
    let mut s = format!("{} · {}", app.branch(), app.comparison_label());
    if app.context_lines() > 3 {
        s.push_str(&format!(" · ⊕{}", app.context_lines()));
    }
    if app.is_watching() {
        s.push_str(" · ◉");
    }
    if app.is_loading() {
        s.push_str(" · ⟳");
    }
    s
}

/// The keybar line (search prompt / error / per-tab hints).
fn keybar_line(app: &App) -> Line<'static> {
    if app.repo_search().is_some() {
        return Line::from(Span::styled(
            "↑/↓ select · Enter open · Tab switch mode · Esc close",
            Style::default().fg(Color::Yellow),
        ));
    }
    if app.ref_picker().is_some() {
        return Line::from(Span::styled(
            "type to filter · ↑/↓ select · Enter compare · Esc close",
            Style::default().fg(Color::Yellow),
        ));
    }
    match (app.search(), app.error_message()) {
        (Some(s), _) if s.editing => Line::from(Span::styled(
            format!("/{}", s.query),
            Style::default().fg(Color::Yellow),
        )),
        (Some(s), _) => Line::from(Span::styled(
            format!("/{}   n/N next·prev · Esc close", s.query),
            Style::default().fg(Color::Yellow),
        )),
        (None, Some(err)) => Line::from(Span::styled(
            format!("⚠ {err}"),
            Style::default().fg(Color::Red),
        )),
        (None, None) => match app.tab() {
            Tab::Diff => keybar_hints(KEYBAR),
            Tab::Files => keybar_hints(KEYBAR_FILES),
        },
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
            "expand/collapse tree; Enter on a file → diff; Enter in diff expands a fold",
        ),
        ("g / G, Ctrl-d/u", "top / bottom; half-page"),
        ("s", "toggle unified / side-by-side"),
        ("w", "toggle word-level highlight"),
        ("o", "expand context (cycle 3→10→30→100)"),
        ("Space", "mark file reviewed"),
        ("c", "compare picker"),
        ("b", "compare vs branch/tag (fuzzy)"),
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

/// The left panel: the changed-file tree, titled with the totals + reviewed count.
fn render_file_list(app: &App, f: &mut Frame, area: Rect) {
    let (add, del) = app.totals();
    let n = app.files().len();
    let title = format!(" Changes  +{add} −{del}  {}/{n} ", app.reviewed_count());
    let block = Block::bordered()
        .title(title)
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
    let palette = Palette::for_mode(mode);
    let word_on = app.config().word_diff;
    // Per-file syntax highlight spans, computed with carried state across the
    // whole file (M12) and cached — so block comments / multi-line strings are
    // correct in the diff, not just the Files tab.
    let (old_hl, new_hl) = app.diff_highlight();
    // Folded display rows (per-gap expand) over the file's full line list.
    let full = app.full_lines();
    let display = app.display_rows();
    // Highlight search matches in the diff while a (non-empty) query is active.
    let query = app
        .search()
        .map(|s| s.query.as_str())
        .filter(|q| !q.is_empty());
    match app.view() {
        ViewMode::Unified => viewer_unified::render(
            f,
            diff_body,
            &full,
            &display,
            app.scroll(),
            &old_hl,
            &new_hl,
            &palette,
            word_on,
            query,
        ),
        ViewMode::SideBySide => viewer_split::render(
            f,
            diff_body,
            &full,
            &display,
            app.scroll(),
            &old_hl,
            &new_hl,
            &palette,
            word_on,
            query,
        ),
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
