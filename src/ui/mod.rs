//! Frame layout and rendering.
//!
//! Render functions take immutable [`App`] state and draw into a ratatui `Frame`
//! with no I/O, so they're snapshot-testable on `TestBackend` (see docs/testing.md).

pub mod browser;
pub mod tree;
pub mod viewer_split;
pub mod viewer_unified;
pub mod viewport;

use crate::app::{App, Focus, Tab};
use crate::config::ViewMode;
use crate::diff::FileDiff;
use crate::highlight::{FgSpan, Palette};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

/// Width of the file tree panel, in columns. Shared by the Diff tree and the
/// Repo browser so the two tabs are visually identical.
pub(crate) const TREE_WIDTH: u16 = 34;

/// Essential key hints for the keybar (key, label). The full list lives in `?`,
/// so this stays a single, uncluttered line that fits a narrow (half) terminal.
const KEYBAR: &[(&str, &str)] = &[
    ("n/p", "file"),
    ("]/[", "hunk"),
    ("Space", "review"),
    ("s", "split"),
    ("/", "find"),
    ("?", "help"),
];

/// Essential key hints for the Files tab.
const KEYBAR_FILES: &[(&str, &str)] = &[
    ("Enter", "open"),
    ("h/l", "in/out"),
    ("/", "find"),
    ("f/F", "finder"),
    ("v/y", "copy"),
    ("?", "help"),
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

/// One row of a file tree, shared by the Diff tree and the Repo browser so both
/// tabs use one compact layout. The status marker sits **inline, right before
/// the name** and indents with the tree (directories sit flush-left):
///
/// ```text
/// ▾ src/app/        (dir: indent + arrow + name)
///     M mod.rs      (Diff file: indent + review + marker + name)
///   M README.md
///   M lib.rs        (Repo file: indent + marker + name — no review column)
/// ```
///
/// * `review` — the Diff review cell (`✓`/`⚠`/blank); `None` omits the column
///   entirely (the Repo tab has no review state).
/// * `marker` — the `M`/`A`/`D`/`R` change marker for a file; `None` renders two
///   blank columns so file names still line up.
/// * `arrow` — `Some(expanded)` for a directory (`▾ `/`▸ ` + trailing `/`),
///   `None` for a file (gets the review/marker gutter instead).
/// * `label_color` — colors the arrow+name; dropped on the selected row so the
///   reversed highlight stays uniform.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tree_row_line(
    width: usize,
    selected: bool,
    depth: usize,
    review: Option<(char, Color)>,
    marker: Option<(char, Color)>,
    arrow: Option<bool>,
    label: &str,
    label_color: Option<Color>,
) -> Line<'static> {
    let row_style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    // On the selected row we drop fg colors: a colored fg would invert into a
    // colored background block under REVERSED.
    let colored = |c: Color| {
        if selected {
            row_style
        } else {
            row_style.fg(c)
        }
    };
    let body_style = match (selected, label_color) {
        (true, _) | (false, None) => row_style,
        (false, Some(c)) => row_style.fg(c),
    };

    let mut spans = vec![Span::styled("  ".repeat(depth), row_style)];
    match arrow {
        Some(expanded) => {
            // Directory: just the arrow + name, flush against the indent.
            spans.push(Span::styled(
                format!("{} {label}/", if expanded { '▾' } else { '▸' }),
                body_style,
            ));
        }
        None => {
            // File: optional review cell, then the 2-wide marker, then the name.
            if let Some((glyph, color)) = review {
                spans.push(Span::styled(format!("{glyph} "), colored(color)));
            }
            match marker {
                Some((ch, color)) => spans.push(Span::styled(format!("{ch} "), colored(color))),
                None => spans.push(Span::styled("  ".to_string(), row_style)),
            }
            spans.push(Span::styled(label.to_string(), body_style));
        }
    }
    // Pad to the panel width so the selected row's highlight spans the panel.
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if used < width {
        spans.push(Span::styled(" ".repeat(width - used), row_style));
    }
    Line::from(spans)
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
        Tab::Diff if app.show_tree() => {
            let [tree_area, viewer] =
                Layout::horizontal([Constraint::Length(TREE_WIDTH), Constraint::Min(0)])
                    .areas(body);
            render_file_list(app, f, tree_area);
            render_viewer(app, f, viewer);
        }
        Tab::Diff => render_viewer(app, f, body), // tree hidden → full-width diff
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
    let scope = match rs.scope {
        crate::app::SearchScope::DiffFiles => "changed",
        crate::app::SearchScope::Repo => "repo",
    };
    let title = format!(
        "Find {scope} [{}] · {} results{} · Tab: switch mode",
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

/// The gitui-style header: tabs `Diff [1]  Repo [2]` left, branch/comparison
/// (with a `→` to the compared-against ref) right.
fn render_header(app: &App, f: &mut Frame, area: Rect) {
    // Tabs on the left, each with its `[n]` switch key.
    let mut spans = vec![Span::raw(" ")];
    let mut used = 1usize;
    for (tab, label, num) in [(Tab::Diff, "Diff", '1'), (Tab::Files, "Repo", '2')] {
        let active = app.tab() == tab;
        let style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::styled(
            format!(" [{num}]"),
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::raw("   "));
        used += label.chars().count() + 4 + 3; // label + " [n]" + gap
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
    // When comparing against a ref, show `branch → ref` (the arrow points at what
    // we diff against); otherwise `branch · comparison` (uncommitted, PR, …).
    let mut s = match app.compare_against() {
        Some(target) => format!("{} → {target}", app.branch()),
        None => format!("{} · {}", app.branch(), app.comparison_label()),
    };
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
    // Quickfix (repo-wide content matches from `F`): position + n/N across files.
    if let Some(qf) = app.quickfix() {
        return Line::from(Span::styled(
            format!(
                "/{}  [{}/{}] across repo · n/N next·prev · Esc close",
                qf.query,
                qf.idx + 1,
                qf.matches.len(),
            ),
            Style::default().fg(Color::Yellow),
        ));
    }
    match (app.search(), app.error_message()) {
        (Some(s), _) if s.editing => Line::from(Span::styled(
            format!("/{}", s.query),
            Style::default().fg(Color::Yellow),
        )),
        (Some(s), _) => {
            let pos = match app.search_match_position() {
                Some((i, n)) => format!("  [{i}/{n}]"),
                None => match app.search_match_count() {
                    0 => "  [no matches]".to_string(),
                    n => format!("  [{n} matches]"),
                },
            };
            Line::from(Span::styled(
                format!("/{}{pos}   n/N next·prev · Esc close", s.query),
                Style::default().fg(Color::Yellow),
            ))
        }
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
        ("j / k, ↑ / ↓", "move / scroll"),
        ("Tab", "switch tree ↔ diff focus"),
        ("n / p", "next / prev file"),
        ("] / [", "next / prev hunk"),
        ("g / G", "top / bottom"),
        ("Ctrl-d / u", "half-page down / up"),
        ("Enter", "open file · expand fold"),
        ("h / l, ← / →", "enter / exit file · collapse / expand dir"),
        ("s", "unified / side-by-side"),
        ("w", "word-level highlight"),
        ("o", "context window 3→10→30→100"),
        ("Space", "mark reviewed"),
        ("c / b", "compare · branch/tag picker"),
        ("/", "find in view: diff / open file (n / N)"),
        ("f / F", "find by name / contents (diff or repo)"),
        ("v / y", "select lines / copy (diff & preview)"),
        ("e", "open in $EDITOR"),
        ("r / a", "refresh · auto-refresh"),
        ("1 / 2", "Diff / Files tab"),
        ("z", "hide / show the tree panel"),
        ("? / q", "help · quit"),
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

/// A friendly, centered placeholder when there's no diff to show — either the
/// comparison is empty, or gandr was opened outside a git repo (files-only).
fn render_empty_state(app: &App, f: &mut Frame, area: Rect) {
    let hint = |k: &'static str, label: &'static str| {
        Line::from(vec![
            Span::styled(
                format!("{k:>5}  "),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(label, Style::default().fg(Color::Gray)),
        ])
        .alignment(ratatui::layout::Alignment::Left)
    };
    let (title, title_color, subtitle, hints) = if app.files_only() {
        (
            "Not a git repository",
            Color::Yellow,
            "browse, preview and search files in the Repo tab".to_string(),
            vec![
                hint("2", "browse the repository"),
                hint("/", "search files & contents"),
                hint("q", "quit"),
            ],
        )
    } else {
        // Say *which* comparison is empty, so the header isn't the only place
        // that tells you what you're looking at.
        let subtitle = match app.compare_against() {
            Some(r) => format!("working tree matches {r}"),
            None => match app.comparison_label().as_str() {
                "uncommitted" => "your working tree is clean".to_string(),
                "staged" => "nothing is staged".to_string(),
                _ => "this comparison is empty".to_string(),
            },
        };
        (
            "✓  No changes to review",
            Color::Green,
            subtitle,
            vec![
                hint("c", "compare against…"),
                hint("b", "pick a branch or tag"),
                hint("2", "browse the repository"),
                hint("r", "refresh"),
            ],
        )
    };
    let subtitle_w = subtitle.chars().count() as u16;
    let mut lines = vec![
        Line::from(Span::styled(
            title,
            Style::default()
                .fg(title_color)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(ratatui::layout::Alignment::Center),
        Line::from(Span::styled(subtitle, Style::default().fg(Color::DarkGray)))
            .alignment(ratatui::layout::Alignment::Center),
        Line::raw(""),
    ];
    lines.extend(hints);
    // Center the block vertically; keep a left-aligned hint column readable.
    let h = lines.len() as u16;
    let y = area.y + area.height.saturating_sub(h) / 2;
    // Fit the box to its widest line (the subtitle grows with the ref name) so
    // the comparison never truncates; clamp to the available width.
    let box_w = (subtitle_w + 4).max(30).min(area.width);
    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let rect = Rect {
        x,
        y,
        width: box_w,
        height: h.min(area.height),
    };
    f.render_widget(Paragraph::new(lines), rect);
}

/// The right panel: a sticky file header plus the scrollable diff body.
fn render_viewer(app: &App, f: &mut Frame, area: Rect) {
    let block = Block::bordered().border_style(border_style(app.focus() == Focus::Diff));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(file) = app.current() else {
        render_empty_state(app, f, inner);
        app.set_viewport(0);
        return;
    };

    let [file_header, diff_body] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);

    f.render_widget(file_header_line(app, file), file_header);
    app.set_viewport(diff_body.height as usize);

    // Binary or very large files have no inline text diff.
    if file.change.is_binary {
        f.render_widget(
            Paragraph::new(Span::styled(
                "No text diff (binary or very large file).",
                Style::default().fg(Color::DarkGray),
            )),
            diff_body,
        );
        return;
    }

    let mode = app.theme_mode();
    let palette = Palette::for_mode(mode);
    let word_on = app.config().word_diff;
    // Cached, multi-line-aware syntax spans (M12), filled by a background job.
    // Until it lands the slices are empty and the viewer renders plain
    // foreground — so selecting a large file never blocks on syntect.
    let highlight = app.diff_highlight();
    let (old_hl, new_hl): (&[Vec<FgSpan>], &[Vec<FgSpan>]) = match &highlight {
        Some((o, n)) => (o, n),
        None => (&[], &[]),
    };
    // Folded display rows (per-gap expand) over the file's full line list.
    let full = app.full_lines();
    let display = app.display_rows();
    // The cursor marks the current line; the viewport follows it.
    let cursor = app.diff_cursor();
    let scroll = app.diff_scroll(diff_body.height as usize);
    // Highlight search matches in the diff while a (non-empty) query is active.
    let query = app
        .search()
        .map(|s| s.query.as_str())
        .filter(|q| !q.is_empty());
    let focused = app.focus() == Focus::Diff;
    match app.view() {
        ViewMode::Unified => viewer_unified::render(
            f,
            diff_body,
            &full,
            &display,
            scroll,
            cursor,
            focused,
            old_hl,
            new_hl,
            &palette,
            word_on,
            query,
            app.diff_selection(),
        ),
        ViewMode::SideBySide => viewer_split::render(
            f,
            diff_body,
            &full,
            &display,
            scroll,
            cursor,
            focused,
            old_hl,
            new_hl,
            &palette,
            word_on,
            query,
            app.diff_selection(),
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
