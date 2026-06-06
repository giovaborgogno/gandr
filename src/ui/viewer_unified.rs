//! Unified (single-column) diff viewer rendering.
//!
//! One ratatui [`Line`] per diff row — hunk headers and diff lines with old/new
//! line-number gutters, a colored change bar, syntax-highlighted text over a
//! delta-style diff background, and word-level emphasis. Renders the window
//! `[scroll, scroll+height)` with the background filled to the panel edge.

use crate::diff::fold::DiffRow;
use crate::diff::{Line as DiffLine, LineKind};
use crate::highlight::{compose, FgSpan, Palette};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Compute the gutter width (digits) from the largest line number in the file.
pub(crate) fn gutter_width(full: &[DiffLine]) -> usize {
    full.iter()
        .filter_map(|l| l.old_no.max(l.new_no))
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
        .max(2)
}

/// The "⋯ N unchanged lines ⋯" marker shown for a collapsed fold; pressing Enter
/// (with the diff focused) expands the one nearest the top of the viewport.
pub(crate) fn fold_marker(hidden: usize, width: usize) -> Line<'static> {
    let label = format!(" ⋯ {hidden} unchanged lines · Enter to expand ⋯");
    let mut text = label.chars().take(width).collect::<String>();
    let used = text.chars().count();
    if used < width {
        text.push_str(&" ".repeat(width - used));
    }
    Line::from(Span::styled(
        text,
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
    ))
}

fn num_cell(no: Option<u32>, width: usize) -> String {
    match no {
        Some(n) => format!("{n:>width$}"),
        None => " ".repeat(width),
    }
}

/// Cached syntax spans for a line: a deleted line reads from the old side
/// (`old_no`), everything else from the new side (`new_no`). Empty when the
/// highlight job hasn't produced this file's spans yet — the line then renders
/// with plain foreground (diff backgrounds still apply), so selecting a large
/// file never blocks on syntect; the colors fill in when the job lands.
pub(crate) fn line_fg<'a>(
    line: &DiffLine,
    old_hl: &'a [Vec<FgSpan>],
    new_hl: &'a [Vec<FgSpan>],
) -> &'a [FgSpan] {
    // `checked_sub`: line numbers are 1-based, but never trust a `- 1` to a panic.
    let spans = match line.kind {
        LineKind::Del => line
            .old_no
            .and_then(|n| (n as usize).checked_sub(1))
            .and_then(|i| old_hl.get(i)),
        _ => line
            .new_no
            .and_then(|n| (n as usize).checked_sub(1))
            .and_then(|i| new_hl.get(i)),
    };
    spans.map(Vec::as_slice).unwrap_or(&[])
}

/// Build a styled [`Line`] for one diff line, filling the background to `width`.
fn line_row(
    line: &DiffLine,
    w: usize,
    width: usize,
    fg: &[FgSpan],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
) -> Line<'static> {
    let (bar, bar_color, base_bg) = match line.kind {
        LineKind::Add => ('▌', Color::Green, Some(palette.add_bg)),
        LineKind::Del => ('▌', Color::Red, Some(palette.del_bg)),
        LineKind::Context => (' ', Color::DarkGray, None),
    };

    let gutter = format!("{} {} ", num_cell(line.old_no, w), num_cell(line.new_no, w));
    let mut spans = vec![
        Span::styled(gutter, Style::default().fg(Color::DarkGray)),
        Span::styled(bar.to_string(), Style::default().fg(bar_color)),
        Span::raw(" "),
    ];

    let text_spans = compose::line_spans(
        &line.text,
        line.kind,
        &line.segments,
        fg,
        palette,
        word_on,
        query,
    );
    spans.extend(text_spans);

    // Fill the rest of the row with the base background, delta-style.
    if let Some(bg) = base_bg {
        let used = w * 2 + 3 + line.text.chars().count(); // gutter + bar + space + text
        if used < width {
            spans.push(Span::styled(
                " ".repeat(width - used),
                Style::default().bg(bg),
            ));
        }
    }
    Line::from(spans)
}

/// Build every display row of the file's folded diff as a styled [`Line`].
/// Build one display row (a fold marker or a diff line) as a styled [`Line`].
#[allow(clippy::too_many_arguments)]
fn build_row(
    row: &DiffRow,
    full: &[DiffLine],
    w: usize,
    width: usize,
    old_hl: &[Vec<FgSpan>],
    new_hl: &[Vec<FgSpan>],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
) -> Line<'static> {
    match row {
        DiffRow::Fold { hidden, .. } => fold_marker(*hidden, width),
        // `.get` rather than `full[*idx]`: indices come from a coherent display
        // cache today, but a graceful empty row beats a panic if that ever slips.
        DiffRow::Line(idx) => match full.get(*idx) {
            Some(line) => {
                let fg = line_fg(line, old_hl, new_hl);
                line_row(line, w, width, fg, palette, word_on, query)
            }
            None => Line::from(""),
        },
    }
}

/// Build every display row (used by tests/benches; `render` windows to the
/// visible slice so per-frame cost stays O(viewport), not O(file)).
#[allow(clippy::too_many_arguments)]
pub fn rows(
    full: &[DiffLine],
    display: &[DiffRow],
    width: usize,
    old_hl: &[Vec<FgSpan>],
    new_hl: &[Vec<FgSpan>],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
) -> Vec<Line<'static>> {
    let w = gutter_width(full);
    display
        .iter()
        .map(|row| build_row(row, full, w, width, old_hl, new_hl, palette, word_on, query))
        .collect()
}

/// Render the unified diff for `file` into `area`, scrolled to `scroll` rows.
///
/// `scroll` is clamped to the last full screen so a stale/over-large value still
/// shows content rather than a blank panel.
#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    full: &[DiffLine],
    display: &[DiffRow],
    scroll: usize,
    old_hl: &[Vec<FgSpan>],
    new_hl: &[Vec<FgSpan>],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
) {
    // Reserve the rightmost column for the scrollbar.
    let content = Rect {
        width: area.width.saturating_sub(1),
        ..area
    };
    // One display row == one terminal row here, so we build only the visible
    // window — per-frame cost is O(viewport), independent of file size.
    let total = display.len();
    let height = area.height as usize;
    let effective = scroll.min(total.saturating_sub(height));
    let w = gutter_width(full);
    let width = content.width as usize;
    let visible: Vec<Line> = display[effective..(effective + height).min(total)]
        .iter()
        .map(|row| build_row(row, full, w, width, old_hl, new_hl, palette, word_on, query))
        .collect();
    f.render_widget(Paragraph::new(visible), content);
    super::render_scrollbar(f, area, total, effective);
}
