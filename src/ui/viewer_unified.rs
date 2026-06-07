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

/// Background for a visual selection (v/y), shared with the Repo preview.
pub(crate) const SELECTION_BG: Color = Color::Rgb(45, 55, 78);

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

/// Right-aligned line-number cell (blank when there's no number on this side).
pub(crate) fn num_cell(no: Option<u32>, width: usize) -> String {
    match no {
        Some(n) => format!("{n:>width$}"),
        None => " ".repeat(width),
    }
}

/// The add/del background color for a line kind (None for context). Shared with
/// the side-by-side viewer.
pub(crate) fn base_bg(kind: LineKind, palette: &Palette) -> Option<Color> {
    match kind {
        LineKind::Add => Some(palette.add_bg),
        LineKind::Del => Some(palette.del_bg),
        LineKind::Context => None,
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

/// The fixed left-margin width of a diff row: two line-number columns, a space
/// after each, the change sign, and a trailing space.
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

/// Build a diff line as one-or-more wrapped terminal rows. The add/del
/// background spans the *whole* row (line-number gutter included); a `+`/`-`
/// sign leads the first row (blank on continuation rows); each row fills to
/// `width`. `is_cursor` reverses the line-number gutter ("you are here").
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
    selected: bool,
) -> Vec<Line<'static>> {
    let (sign, sign_color) = match line.kind {
        LineKind::Add => ('+', Color::Green),
        LineKind::Del => ('-', Color::Red),
        LineKind::Context => (' ', Color::DarkGray),
    };
    // A visual selection (v/y) overrides the add/del background so the selected
    // run reads as one block; the +/- sign colors still mark each line's kind.
    let base_bg = if selected {
        Some(SELECTION_BG)
    } else {
        base_bg(line.kind, palette)
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

    // The row's base background, applied to every prefix span so the color runs
    // edge to edge (gutter included).
    let bg = |style: Style| match base_bg {
        Some(c) => style.bg(c),
        None => style,
    };
    let mut out = Vec::with_capacity(wrapped.len().max(1));
    for (k, chunk) in wrapped.iter().enumerate() {
        let (gutter, sign_ch) = if k == 0 {
            (
                format!("{} {} ", num_cell(line.old_no, w), num_cell(line.new_no, w)),
                sign,
            )
        } else {
            (" ".repeat(2 * w + 2), ' ') // continuation: blank numbers, no sign
        };
        let mut gutter_style = bg(Style::default().fg(Color::DarkGray));
        if is_cursor && k == 0 {
            gutter_style = gutter_style.add_modifier(Modifier::REVERSED);
        }
        let mut spans = vec![
            Span::styled(gutter, gutter_style),
            Span::styled(
                sign_ch.to_string(),
                bg(Style::default().fg(sign_color).add_modifier(Modifier::BOLD)),
            ),
            Span::styled(" ", bg(Style::default())),
        ];
        spans.extend(chunk.iter().cloned());
        if let Some(c) = base_bg {
            let used = prefix
                + chunk
                    .iter()
                    .map(|s| s.content.chars().count())
                    .sum::<usize>();
            if used < width {
                spans.push(Span::styled(
                    " ".repeat(width - used),
                    Style::default().bg(c),
                ));
            }
        }
        // The composed text spans carry their own (diff/syntax) background, so the
        // selection has to be stamped over every span — otherwise only the gutter
        // and padding show it and a context line's code stays unpainted.
        if selected {
            spans = spans
                .into_iter()
                .map(|s| Span::styled(s.content, s.style.bg(SELECTION_BG)))
                .collect();
        }
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(Line::from(""));
    }
    out
}

/// Build the terminal rows for one display row (a fold marker, or a diff line
/// wrapped into one-or-more rows).
#[allow(clippy::too_many_arguments)]
fn display_row_lines(
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
    selected: bool,
) -> Vec<Line<'static>> {
    match row {
        DiffRow::Fold { hidden, .. } => vec![fold_marker(*hidden, width, is_cursor)],
        // `.get` rather than `full[*idx]`: indices come from a coherent display
        // cache today, but a graceful empty row beats a panic if that ever slips.
        DiffRow::Line(idx) => match full.get(*idx) {
            Some(line) => {
                let fg = line_fg(line, old_hl, new_hl);
                line_rows_wrapped(
                    line, w, width, fg, palette, word_on, query, is_cursor, selected,
                )
            }
            None => vec![Line::from("")],
        },
    }
}

/// Build every terminal row of the file's folded diff (used by tests/benches;
/// `render` windows to the visible slice so per-frame cost stays O(viewport)).
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
        .flat_map(|row| {
            display_row_lines(
                row, full, w, width, old_hl, new_hl, palette, word_on, query, false, false,
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
    selection: Option<(usize, usize)>,
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
        let selected = selection.is_some_and(|(lo, hi)| di >= lo && di <= hi);
        for row in display_row_lines(
            &display[di],
            full,
            w,
            width,
            old_hl,
            new_hl,
            palette,
            word_on,
            query,
            is_cursor,
            selected,
        ) {
            if term_rows.len() < height {
                term_rows.push(row);
            }
        }
        di += 1;
    }
    f.render_widget(Paragraph::new(term_rows), content);
    // Scrollbar is approximated in display rows (cheap; exact terminal-row counts
    // would require measuring the whole file).
    super::render_scrollbar(f, area, total, top);
}
