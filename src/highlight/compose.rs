//! Compose a diff line's TEXT into styled ratatui spans by layering three signals:
//! syntax foreground, the diff background (add/del), and a stronger background on
//! word-level changed segments. The gutter and hunk bar are handled by the viewer.

use super::{FgSpan, Palette};
use crate::diff::{LineKind, Segment};
use ratatui::style::Style;
use ratatui::text::Span;
use std::collections::BTreeSet;

/// Build the styled spans for a line's text content.
pub fn line_spans(
    text: &str,
    kind: LineKind,
    segments: &[Segment],
    fg: &[FgSpan],
    palette: &Palette,
    word_on: bool,
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return Vec::new();
    }

    let (base_bg, strong_bg) = match kind {
        LineKind::Add => (Some(palette.add_bg), Some(palette.add_strong_bg)),
        LineKind::Del => (Some(palette.del_bg), Some(palette.del_strong_bg)),
        LineKind::Context => (None, None),
    };

    // Cut the line at every fg-span and segment boundary; each resulting slice
    // has a single (fg, bg) style.
    let len = text.len();
    let mut bounds = BTreeSet::new();
    bounds.insert(0);
    bounds.insert(len);
    for s in fg {
        bounds.insert(s.start.min(len));
        bounds.insert(s.end.min(len));
    }
    for s in segments {
        bounds.insert(s.start.min(len));
        bounds.insert(s.end.min(len));
    }
    let points: Vec<usize> = bounds.into_iter().collect();

    let mut spans = Vec::with_capacity(points.len());
    for win in points.windows(2) {
        let (a, b) = (win[0], win[1]);
        if a >= b {
            continue;
        }
        let color = fg
            .iter()
            .find(|s| s.start <= a && a < s.end)
            .map(|s| s.color);
        let changed = word_on && segments.iter().any(|s| s.start <= a && a < s.end);
        let bg = if changed { strong_bg } else { base_bg };

        let mut style = Style::default();
        if let Some(c) = color {
            style = style.fg(c);
        }
        if let Some(bg) = bg {
            style = style.bg(bg);
        }
        spans.push(Span::styled(text[a..b].to_string(), style));
    }
    spans
}
