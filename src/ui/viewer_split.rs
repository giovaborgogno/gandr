//! Side-by-side (two-column) diff viewer: old on the left, new on the right.
//!
//! Removed/added lines are paired by position; long lines **wrap** within their
//! column, and a logical row expands to the taller of its two wrapped cells so
//! the two sides stay aligned (the user-chosen behavior over truncation).

use crate::diff::{FileDiff, Hunk, Line as DiffLine, LineKind};
use crate::highlight::{compose, Highlighter, Palette};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Gutter (line-number) width for a side.
fn gutter_width(file: &FileDiff) -> usize {
    file.hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter_map(|l| l.old_no.max(l.new_no))
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
        .max(2)
}

fn num_cell(no: Option<u32>, width: usize) -> String {
    match no {
        Some(n) => format!("{n:>width$}"),
        None => " ".repeat(width),
    }
}

/// Wrap styled spans to `width` display columns (char-count approximation),
/// preserving each span's style across the split.
fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
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

fn base_bg(kind: LineKind, palette: &Palette) -> Option<Color> {
    match kind {
        LineKind::Add => Some(palette.add_bg),
        LineKind::Del => Some(palette.del_bg),
        LineKind::Context => None,
    }
}

/// Render one cell (one diff line on one side) into wrapped rows of spans, each
/// padded to `side_w` columns. `None` cell → blank rows.
fn cell_rows(
    line: Option<&DiffLine>,
    side_w: usize,
    gutter_w: usize,
    hl: &Highlighter,
    palette: &Palette,
    word_on: bool,
) -> Vec<Vec<Span<'static>>> {
    let text_w = side_w.saturating_sub(gutter_w + 1).max(1);
    let Some(line) = line else {
        return vec![vec![Span::raw(" ".repeat(side_w))]];
    };

    let fg = hl.fg_spans(&line.text);
    let composed =
        compose::line_spans(&line.text, line.kind, &line.segments, &fg, palette, word_on);
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
        let mut row = vec![Span::styled(
            format!("{gutter} "),
            Style::default().fg(Color::DarkGray),
        )];
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
        rows.push(row);
    }
    rows
}

/// Pair a hunk's lines into (old, new) logical rows for the two columns.
fn pair_rows(hunk: &Hunk) -> Vec<(Option<&DiffLine>, Option<&DiffLine>)> {
    let mut pairs = Vec::new();
    let lines = &hunk.lines;
    let mut i = 0;
    while i < lines.len() {
        match lines[i].kind {
            LineKind::Context => {
                pairs.push((Some(&lines[i]), Some(&lines[i])));
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
                    pairs.push((dels.get(k), adds.get(k)));
                }
            }
        }
    }
    pairs
}

/// Build every terminal row of the side-by-side view.
pub fn rows(
    file: &FileDiff,
    width: usize,
    hl: &Highlighter,
    palette: &Palette,
    word_on: bool,
) -> Vec<Line<'static>> {
    let gutter_w = gutter_width(file);
    let side_w = width.saturating_sub(1) / 2;
    let mut out: Vec<Line<'static>> = Vec::new();

    for hunk in &file.hunks {
        out.push(Line::from(Span::styled(
            hunk.header.clone(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
        )));

        for (left, right) in pair_rows(hunk) {
            let left_rows = cell_rows(left, side_w, gutter_w, hl, palette, word_on);
            let right_rows = cell_rows(right, side_w, gutter_w, hl, palette, word_on);
            let height = left_rows.len().max(right_rows.len());

            for k in 0..height {
                let mut spans = left_rows
                    .get(k)
                    .cloned()
                    .unwrap_or_else(|| vec![Span::raw(" ".repeat(side_w))]);
                spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
                spans.extend(
                    right_rows
                        .get(k)
                        .cloned()
                        .unwrap_or_else(|| vec![Span::raw(" ".repeat(side_w))]),
                );
                out.push(Line::from(spans));
            }
        }
    }
    out
}

/// Render the side-by-side diff for `file` into `area`, scrolled to `scroll`.
pub fn render(
    f: &mut Frame,
    area: Rect,
    file: &FileDiff,
    scroll: usize,
    hl: &Highlighter,
    palette: &Palette,
    word_on: bool,
) {
    let all = rows(file, area.width as usize, hl, palette, word_on);
    let height = area.height as usize;
    let max_scroll = all.len().saturating_sub(height);
    let effective = scroll.min(max_scroll);
    let visible: Vec<Line> = all.into_iter().skip(effective).take(height).collect();
    f.render_widget(Paragraph::new(visible), area);
}
