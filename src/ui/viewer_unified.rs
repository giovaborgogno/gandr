//! Unified (single-column) diff viewer rendering.
//!
//! Builds one ratatui [`Line`] per diff row — hunk headers and `+/-/ ` lines with
//! old/new line-number gutters — then renders the window `[scroll, scroll+height)`.
//! Delta-style backgrounds, word-level emphasis and syntax highlighting arrive in
//! M3; M2 keeps it to structure + simple foreground colors.

use crate::diff::{FileDiff, LineKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Compute the gutter width (digits) from the largest line number in the file.
fn gutter_width(file: &FileDiff) -> usize {
    let max_no = file
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter_map(|l| l.old_no.max(l.new_no))
        .max()
        .unwrap_or(1);
    max_no.to_string().len().max(2)
}

fn num_cell(no: Option<u32>, width: usize) -> String {
    match no {
        Some(n) => format!("{n:>width$}"),
        None => " ".repeat(width),
    }
}

/// Build every row of the file's diff as a styled [`Line`].
pub fn rows(file: &FileDiff) -> Vec<Line<'static>> {
    let w = gutter_width(file);
    let mut out: Vec<Line<'static>> = Vec::new();

    for hunk in &file.hunks {
        out.push(Line::from(Span::styled(
            hunk.header.clone(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
        )));

        for line in &hunk.lines {
            let (sign, fg) = match line.kind {
                LineKind::Add => ('+', Color::Green),
                LineKind::Del => ('-', Color::Red),
                LineKind::Context => (' ', Color::Reset),
            };
            let gutter = format!(
                "{} {} {} ",
                num_cell(line.old_no, w),
                num_cell(line.new_no, w),
                sign
            );
            out.push(Line::from(vec![
                Span::styled(gutter, Style::default().fg(Color::DarkGray)),
                Span::styled(line.text.clone(), Style::default().fg(fg)),
            ]));
        }
    }
    out
}

/// Render the unified diff for `file` into `area`, scrolled to `scroll` rows.
///
/// `scroll` is clamped to the last full screen so a stale/over-large value (e.g.
/// `G` pressed before the first render set the viewport height) still shows
/// content rather than a blank panel.
pub fn render(f: &mut Frame, area: Rect, file: &FileDiff, scroll: usize) {
    let all = rows(file);
    let height = area.height as usize;
    let max_scroll = all.len().saturating_sub(height);
    let effective = scroll.min(max_scroll);
    let visible: Vec<Line> = all.into_iter().skip(effective).take(height).collect();
    f.render_widget(Paragraph::new(visible), area);
}
