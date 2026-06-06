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
/// (diff focused) expands the one under the cursor. `is_cursor` highlights it.
pub(crate) fn fold_marker(hidden: usize, width: usize, is_cursor: bool) -> Line<'static> {
    let label = format!(" ⋯ {hidden} unchanged lines · Enter to expand ⋯");
    let mut text = label.chars().take(width).collect::<String>();
    let used = text.chars().count();
    if used < width {
        text.push_str(&" ".repeat(width - used));
    }
    let style = if is_cursor {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    };
    Line::from(Span::styled(text, style))
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
/// `is_cursor` marks the current line (its gutter is reversed — "you are here").
#[allow(clippy::too_many_arguments)]
fn line_row(
    line: &DiffLine,
    w: usize,
    width: usize,
    fg: &[FgSpan],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
    is_cursor: bool,
) -> Line<'static> {
    let (bar, bar_color, base_bg) = match line.kind {
        LineKind::Add => ('▌', Color::Green, Some(palette.add_bg)),
        LineKind::Del => ('▌', Color::Red, Some(palette.del_bg)),
        LineKind::Context => (' ', Color::DarkGray, None),
    };

    let gutter = format!("{} {} ", num_cell(line.old_no, w), num_cell(line.new_no, w));
    let gutter_style = if is_cursor {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mut spans = vec![
        Span::styled(gutter, gutter_style),
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

/// The fixed left-margin width of a diff row: two line-number columns, a space
/// after each, the change bar, and a trailing space.
fn prefix_width(w: usize) -> usize {
    2 * w + 4
}

/// How many terminal rows a display row occupies once its text is wrapped to the
/// panel width (1 for a fold marker, which isn't wrapped).
fn row_height(row: &DiffRow, full: &[DiffLine], w: usize, width: usize) -> usize {
    match row {
        DiffRow::Fold { .. } => 1,
        DiffRow::Line(idx) => match full.get(*idx) {
            Some(line) => {
                let text_w = width.saturating_sub(prefix_width(w)).max(1);
                line.text.chars().count().div_ceil(text_w).max(1)
            }
            None => 1,
        },
    }
}

/// Build a diff line as one-or-more wrapped terminal rows: the gutter + change
/// bar lead the first row, continuation rows keep the bar with a blank gutter,
/// and each row's background fills to `width` (delta-style). `is_cursor` reverses
/// the line-number gutter on the first row ("you are here").
#[allow(clippy::too_many_arguments)]
fn line_rows_wrapped(
    line: &DiffLine,
    w: usize,
    width: usize,
    fg: &[FgSpan],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
    is_cursor: bool,
) -> Vec<Line<'static>> {
    let (bar, bar_color, base_bg) = match line.kind {
        LineKind::Add => ('▌', Color::Green, Some(palette.add_bg)),
        LineKind::Del => ('▌', Color::Red, Some(palette.del_bg)),
        LineKind::Context => (' ', Color::DarkGray, None),
    };
    let prefix = prefix_width(w);
    let text_w = width.saturating_sub(prefix).max(1);
    let text_spans = compose::line_spans(
        &line.text,
        line.kind,
        &line.segments,
        fg,
        palette,
        word_on,
        query,
    );
    let wrapped = crate::ui::viewer_split::wrap_spans(&text_spans, text_w);

    let gutter_style = if is_cursor {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mut out = Vec::with_capacity(wrapped.len().max(1));
    for (k, chunk) in wrapped.iter().enumerate() {
        let gutter = if k == 0 {
            format!("{} {} ", num_cell(line.old_no, w), num_cell(line.new_no, w))
        } else {
            " ".repeat(2 * w + 2) // continuation: blank line numbers, keep the bar
        };
        let mut spans = vec![
            Span::styled(gutter, gutter_style),
            Span::styled(bar.to_string(), Style::default().fg(bar_color)),
            Span::raw(" "),
        ];
        spans.extend(chunk.iter().cloned());
        if let Some(bg) = base_bg {
            let used = prefix
                + chunk
                    .iter()
                    .map(|s| s.content.chars().count())
                    .sum::<usize>();
            if used < width {
                spans.push(Span::styled(
                    " ".repeat(width - used),
                    Style::default().bg(bg),
                ));
            }
        }
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(""));
    }
    out
}

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
    is_cursor: bool,
) -> Line<'static> {
    match row {
        DiffRow::Fold { hidden, .. } => fold_marker(*hidden, width, is_cursor),
        // `.get` rather than `full[*idx]`: indices come from a coherent display
        // cache today, but a graceful empty row beats a panic if that ever slips.
        DiffRow::Line(idx) => match full.get(*idx) {
            Some(line) => {
                let fg = line_fg(line, old_hl, new_hl);
                line_row(line, w, width, fg, palette, word_on, query, is_cursor)
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
        .map(|row| {
            build_row(
                row, full, w, width, old_hl, new_hl, palette, word_on, query, false,
            )
        })
        .collect()
}

/// Render the unified diff into `area`, scrolled to `scroll`, with the current
/// line at `cursor` highlighted (when `focused`). Builds only the visible window,
/// so per-frame cost is O(viewport), independent of file size.
#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    full: &[DiffLine],
    display: &[DiffRow],
    scroll: usize,
    cursor: usize,
    focused: bool,
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
    let total = display.len();
    let height = area.height as usize;
    let w = gutter_width(full);
    let width = content.width as usize;

    // Long lines wrap, so a display row may take several terminal rows. Pick the
    // first display row to draw (`top`): start from the viewport-follow scroll,
    // pull up if the cursor is above it, then push down until the cursor's wrapped
    // rows fit in `height` — so the cursor is always visible without measuring
    // the whole (possibly huge) file.
    let mut top = scroll.min(total.saturating_sub(1)).min(cursor);
    while top < cursor {
        let used: usize = (top..=cursor)
            .map(|i| row_height(&display[i], full, w, width))
            .sum();
        if used <= height {
            break;
        }
        top += 1;
    }

    // Build wrapped terminal rows from `top` until the panel is full.
    let mut term_rows: Vec<Line> = Vec::with_capacity(height);
    let mut di = top;
    while di < total && term_rows.len() < height {
        let is_cursor = focused && di == cursor;
        match &display[di] {
            DiffRow::Fold { hidden, .. } => term_rows.push(fold_marker(*hidden, width, is_cursor)),
            DiffRow::Line(idx) => {
                if let Some(line) = full.get(*idx) {
                    let fg = line_fg(line, old_hl, new_hl);
                    for row in
                        line_rows_wrapped(line, w, width, fg, palette, word_on, query, is_cursor)
                    {
                        if term_rows.len() < height {
                            term_rows.push(row);
                        }
                    }
                }
            }
        }
        di += 1;
    }
    f.render_widget(Paragraph::new(term_rows), content);
    // Scrollbar is approximated in display rows (cheap; exact terminal-row counts
    // would require measuring the whole file).
    super::render_scrollbar(f, area, total, top);
}
