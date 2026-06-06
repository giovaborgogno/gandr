//! Unified (single-column) diff viewer rendering.
//!
//! One ratatui [`Line`] per diff row — hunk headers and diff lines with old/new
//! line-number gutters, a colored change bar, syntax-highlighted text over a
//! delta-style diff background, and word-level emphasis. Renders the window
//! `[scroll, scroll+height)` with the background filled to the panel edge.

use crate::diff::{FileDiff, Line as DiffLine, LineKind};
use crate::highlight::{compose, FgSpan, Palette};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Compute the gutter width (digits) from the largest line number in the file.
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

/// The precomputed syntax spans for a line: a deleted line reads from the old
/// side (`old_no`), everything else from the new side (`new_no`). Empty if the
/// line number is out of range (shouldn't happen for a well-formed diff).
pub(crate) fn line_fg<'a>(
    line: &DiffLine,
    old_hl: &'a [Vec<FgSpan>],
    new_hl: &'a [Vec<FgSpan>],
) -> &'a [FgSpan] {
    let spans = match line.kind {
        LineKind::Del => line.old_no.and_then(|n| old_hl.get(n as usize - 1)),
        _ => line.new_no.and_then(|n| new_hl.get(n as usize - 1)),
    };
    spans.map(Vec::as_slice).unwrap_or(&[])
}

/// Build every row of the file's diff as a styled [`Line`], filling backgrounds
/// to `width` columns.
#[allow(clippy::too_many_arguments)]
pub fn rows(
    file: &FileDiff,
    width: usize,
    old_hl: &[Vec<FgSpan>],
    new_hl: &[Vec<FgSpan>],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
) -> Vec<Line<'static>> {
    let w = gutter_width(file);
    let mut out: Vec<Line<'static>> = Vec::new();

    for hunk in &file.hunks {
        out.push(Line::from(Span::styled(
            hunk.header.clone(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
        )));

        for line in &hunk.lines {
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

            let fg = line_fg(line, old_hl, new_hl);
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
                    let pad = " ".repeat(width - used);
                    spans.push(Span::styled(pad, Style::default().bg(bg)));
                }
            }

            out.push(Line::from(spans));
        }
    }
    out
}

/// Render the unified diff for `file` into `area`, scrolled to `scroll` rows.
///
/// `scroll` is clamped to the last full screen so a stale/over-large value still
/// shows content rather than a blank panel.
#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    file: &FileDiff,
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
    let all = rows(
        file,
        content.width as usize,
        old_hl,
        new_hl,
        palette,
        word_on,
        query,
    );
    let total = all.len();
    let height = area.height as usize;
    let effective = scroll.min(total.saturating_sub(height));
    let visible: Vec<Line> = all.into_iter().skip(effective).take(height).collect();
    f.render_widget(Paragraph::new(visible), content);
    super::render_scrollbar(f, area, total, effective);
}
