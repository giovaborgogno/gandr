//! Side-by-side (two-column) diff viewer: old on the left, new on the right.
//!
//! Removed/added lines are paired by position; long lines **wrap** within their
//! column, and a logical row expands to the taller of its two wrapped cells so
//! the two sides stay aligned (the user-chosen behavior over truncation).

use crate::diff::fold::DiffRow;
use crate::diff::{Line as DiffLine, LineKind};
use crate::highlight::{compose, FgSpan, Palette};
use crate::ui::viewer_unified::{
    base_bg, fold_marker, gutter_width, line_fg, num_cell, SELECTION_BG,
};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::collections::HashSet;

/// Wrap styled spans to `width` display columns (char-count approximation),
/// preserving each span's style. Shared with the unified viewer.
pub(crate) fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut cur_w = 0usize;

    for span in spans {
        let style = span.style;
        let mut buf = String::new();
        for ch in span.content.chars() {
            if cur_w == width {
                if !buf.is_empty() {
                    cur.push(Span::styled(std::mem::take(&mut buf), style));
                }
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            buf.push(ch);
            cur_w += 1;
        }
        if !buf.is_empty() {
            cur.push(Span::styled(buf, style));
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Render one cell (one diff line on one side) into wrapped rows of spans, each
/// padded to `side_w` columns. `None` cell → blank rows.
#[allow(clippy::too_many_arguments)]
fn cell_rows(
    line: Option<&DiffLine>,
    side_w: usize,
    gutter_w: usize,
    old_hl: &[Vec<FgSpan>],
    new_hl: &[Vec<FgSpan>],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
    is_cursor: bool,
    selected: bool,
) -> Vec<Vec<Span<'static>>> {
    let text_w = side_w.saturating_sub(gutter_w + 1).max(1);
    let Some(line) = line else {
        // A blank (unpaired) cell still shows the selection background.
        let style = if selected {
            Style::default().bg(SELECTION_BG)
        } else {
            Style::default()
        };
        return vec![vec![Span::styled(" ".repeat(side_w), style)]];
    };

    let fg = line_fg(line, old_hl, new_hl);
    let composed = compose::line_spans(
        &line.text,
        line.kind,
        &line.segments,
        fg,
        palette,
        word_on,
        query,
    );
    let wrapped = wrap_spans(&composed, text_w);
    let num = if line.kind == LineKind::Del {
        line.old_no
    } else {
        line.new_no
    };
    let bg = base_bg(line.kind, palette);

    let mut rows = Vec::with_capacity(wrapped.len());
    for (k, spans) in wrapped.iter().enumerate() {
        let gutter = if k == 0 {
            num_cell(num, gutter_w)
        } else {
            " ".repeat(gutter_w)
        };
        // The line-number gutter reverses on the cursor row ("you are here").
        let mut gutter_style = Style::default().fg(Color::DarkGray);
        if is_cursor && k == 0 {
            gutter_style = gutter_style.add_modifier(Modifier::REVERSED);
        }
        let mut row = vec![Span::styled(format!("{gutter} "), gutter_style)];
        row.extend(spans.iter().cloned());

        // Pad the text area to side_w with the base background.
        let used = gutter_w
            + 1
            + spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>();
        if used < side_w {
            let pad = " ".repeat(side_w - used);
            let mut style = Style::default();
            if let Some(bg) = bg {
                style = style.bg(bg);
            }
            row.push(Span::styled(pad, style));
        }
        // A selected cell paints its whole row (code included), like the unified
        // viewer — stamped over the composed per-span backgrounds.
        if selected {
            row = row
                .into_iter()
                .map(|s| Span::styled(s.content, s.style.bg(SELECTION_BG)))
                .collect();
        }
        rows.push(row);
    }
    rows
}

/// Pair a visible region's lines into (old, new) logical rows for the two columns.
fn pair_rows<'a>(lines: &[&'a DiffLine]) -> Vec<(Option<&'a DiffLine>, Option<&'a DiffLine>)> {
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        match lines[i].kind {
            LineKind::Context => {
                pairs.push((Some(lines[i]), Some(lines[i])));
                i += 1;
            }
            LineKind::Del | LineKind::Add => {
                let dels_start = i;
                while i < lines.len() && lines[i].kind == LineKind::Del {
                    i += 1;
                }
                let dels = &lines[dels_start..i];
                let adds_start = i;
                while i < lines.len() && lines[i].kind == LineKind::Add {
                    i += 1;
                }
                let adds = &lines[adds_start..i];
                for k in 0..dels.len().max(adds.len()) {
                    pairs.push((dels.get(k).copied(), adds.get(k).copied()));
                }
            }
        }
    }
    pairs
}

/// A laid-out row of the side-by-side view, before span building: either a fold
/// marker or a paired (old, new) line. Cheap to produce (references only), so we
/// can count total terminal rows without composing/wrapping any spans.
enum LogicalRow<'a> {
    Fold(usize),
    Pair(Option<&'a DiffLine>, Option<&'a DiffLine>),
}

/// The full logical-row sequence (fold markers + paired lines). No span work.
fn logical_rows<'a>(full: &'a [DiffLine], display: &[DiffRow]) -> Vec<LogicalRow<'a>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < display.len() {
        match &display[i] {
            DiffRow::Fold { hidden, .. } => {
                out.push(LogicalRow::Fold(*hidden));
                i += 1;
            }
            DiffRow::Line(_) => {
                let start = i;
                while i < display.len() && matches!(display[i], DiffRow::Line(_)) {
                    i += 1;
                }
                let region: Vec<&DiffLine> = display[start..i]
                    .iter()
                    .filter_map(|r| match r {
                        DiffRow::Line(idx) => full.get(*idx),
                        DiffRow::Fold { .. } => None,
                    })
                    .collect();
                for (l, r) in pair_rows(&region) {
                    out.push(LogicalRow::Pair(l, r));
                }
            }
        }
    }
    out
}

/// How many terminal rows a cell wraps to (None side → one blank row). Matches
/// `cell_rows(..).len()` but without building any spans.
fn cell_height(line: Option<&DiffLine>, text_w: usize) -> usize {
    match line {
        None => 1,
        Some(l) => l.text.chars().count().div_ceil(text_w).max(1),
    }
}

fn row_height(lr: &LogicalRow, text_w: usize) -> usize {
    match lr {
        LogicalRow::Fold(_) => 1,
        LogicalRow::Pair(l, r) => cell_height(*l, text_w).max(cell_height(*r, text_w)),
    }
}

/// Build the terminal `Line`s for one paired row (old `│` new). `cursor`/`sel`
/// hold the cursor line and the selected-line set (by pointer into `full`), so a
/// cell highlights when its line is the cursor or inside the visual selection.
#[allow(clippy::too_many_arguments)]
fn build_pair(
    left: Option<&DiffLine>,
    right: Option<&DiffLine>,
    side_w: usize,
    gutter_w: usize,
    old_hl: &[Vec<FgSpan>],
    new_hl: &[Vec<FgSpan>],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
    cursor: Option<*const DiffLine>,
    sel: &HashSet<*const DiffLine>,
) -> Vec<Line<'static>> {
    let is_cursor = |c: Option<&DiffLine>| c.is_some_and(|l| cursor == Some(l as *const DiffLine));
    let is_sel = |c: Option<&DiffLine>| c.is_some_and(|l| sel.contains(&(l as *const DiffLine)));
    let (l_sel, r_sel) = (is_sel(left), is_sel(right));
    let left_rows = cell_rows(
        left,
        side_w,
        gutter_w,
        old_hl,
        new_hl,
        palette,
        word_on,
        query,
        is_cursor(left),
        l_sel,
    );
    let right_rows = cell_rows(
        right,
        side_w,
        gutter_w,
        old_hl,
        new_hl,
        palette,
        word_on,
        query,
        is_cursor(right),
        r_sel,
    );
    let height = left_rows.len().max(right_rows.len());
    let blank = |sel: bool| {
        let style = if sel {
            Style::default().bg(SELECTION_BG)
        } else {
            Style::default()
        };
        vec![Span::styled(" ".repeat(side_w), style)]
    };
    let sep_style = if l_sel || r_sel {
        Style::default().fg(Color::DarkGray).bg(SELECTION_BG)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mut out = Vec::with_capacity(height);
    for k in 0..height {
        let mut spans = left_rows.get(k).cloned().unwrap_or_else(|| blank(l_sel));
        spans.push(Span::styled("│", sep_style));
        spans.extend(right_rows.get(k).cloned().unwrap_or_else(|| blank(r_sel)));
        out.push(Line::from(spans));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn build_logical(
    lr: &LogicalRow,
    width: usize,
    side_w: usize,
    gutter_w: usize,
    old_hl: &[Vec<FgSpan>],
    new_hl: &[Vec<FgSpan>],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
    cursor: Option<*const DiffLine>,
    sel: &HashSet<*const DiffLine>,
) -> Vec<Line<'static>> {
    match lr {
        LogicalRow::Fold(hidden) => vec![fold_marker(*hidden, width, false)],
        LogicalRow::Pair(l, r) => build_pair(
            *l, *r, side_w, gutter_w, old_hl, new_hl, palette, word_on, query, cursor, sel,
        ),
    }
}

/// Build *every* terminal row of the side-by-side view (used by tests/benches;
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
    let gutter_w = gutter_width(full);
    let side_w = width.saturating_sub(1) / 2;
    let sel = HashSet::new();
    logical_rows(full, display)
        .iter()
        .flat_map(|lr| {
            build_logical(
                lr, width, side_w, gutter_w, old_hl, new_hl, palette, word_on, query, None, &sel,
            )
        })
        .collect()
}

/// The set of line pointers selected/under-cursor, from display-row indices into
/// `full` — so `build_pair` can match a cell's line by identity.
fn line_ptrs(
    full: &[DiffLine],
    display: &[DiffRow],
    rows: impl Iterator<Item = usize>,
) -> Vec<*const DiffLine> {
    rows.filter_map(|d| match display.get(d) {
        Some(DiffRow::Line(idx)) => full.get(*idx).map(|l| l as *const DiffLine),
        _ => None,
    })
    .collect()
}

/// Render the side-by-side diff into `area`, scrolled to `scroll`. Counts total
/// terminal rows cheaply (no span work) and builds spans only for the visible
/// window, so per-frame cost is O(viewport) regardless of file size.
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
    let content = Rect {
        width: area.width.saturating_sub(1),
        ..area
    };
    let width = content.width as usize;
    let gutter_w = gutter_width(full);
    let side_w = width.saturating_sub(1) / 2;
    let text_w = side_w.saturating_sub(gutter_w + 1).max(1);
    let height = area.height as usize;

    // The cursor line (when focused) and the selected lines, matched by identity
    // in `build_pair` — the diff cursor/selection live in display-row space.
    let cursor_line = focused
        .then(|| match display.get(cursor) {
            Some(DiffRow::Line(idx)) => full.get(*idx).map(|l| l as *const DiffLine),
            _ => None,
        })
        .flatten();
    let sel: HashSet<*const DiffLine> = match selection {
        Some((lo, hi)) => line_ptrs(full, display, lo..=hi).into_iter().collect(),
        None => HashSet::new(),
    };

    let logical = logical_rows(full, display);
    let total: usize = logical.iter().map(|lr| row_height(lr, text_w)).sum();
    let effective = scroll.min(total.saturating_sub(height));

    // Build only the logical rows whose terminal-row range overlaps the window.
    let mut out: Vec<Line> = Vec::with_capacity(height);
    let mut term = 0usize;
    for lr in &logical {
        if term >= effective + height {
            break;
        }
        let h = row_height(lr, text_w);
        if term + h > effective {
            for (k, line) in build_logical(
                lr,
                width,
                side_w,
                gutter_w,
                old_hl,
                new_hl,
                palette,
                word_on,
                query,
                cursor_line,
                &sel,
            )
            .into_iter()
            .enumerate()
            {
                if term + k >= effective && out.len() < height {
                    out.push(line);
                }
            }
        }
        term += h;
    }
    f.render_widget(Paragraph::new(out), content);
    super::render_scrollbar(f, area, total, effective);
}
