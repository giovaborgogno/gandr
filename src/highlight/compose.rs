//! Compose a diff line's TEXT into styled ratatui spans by layering three signals:
//! syntax foreground, the diff background (add/del), and a stronger background on
//! word-level changed segments. The gutter and hunk bar are handled by the viewer.

use super::{FgSpan, Palette};
use crate::diff::{LineKind, Segment};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use std::collections::BTreeSet;

/// Byte ranges of occurrences of `query` in `text`, with **smart case**: the
/// search is case-insensitive unless `query` contains an uppercase ASCII letter,
/// in which case it's case-sensitive. ASCII case-folding preserves byte offsets,
/// so the ranges stay char-aligned.
pub fn match_ranges(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let case_sensitive = query.bytes().any(|b| b.is_ascii_uppercase());
    let fold = |b: u8| {
        if case_sensitive {
            b
        } else {
            b.to_ascii_lowercase()
        }
    };
    let hay: Vec<u8> = text.bytes().map(fold).collect();
    let needle: Vec<u8> = query.bytes().map(fold).collect();
    if needle.len() > hay.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            let end = i + needle.len();
            if text.is_char_boundary(i) && text.is_char_boundary(end) {
                out.push((i, end));
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// Build the styled spans for a line's text content. When `query` is set, its
/// occurrences are highlighted (yellow) on top of everything else.
pub fn line_spans(
    text: &str,
    kind: LineKind,
    segments: &[Segment],
    fg: &[FgSpan],
    palette: &Palette,
    word_on: bool,
    query: Option<&str>,
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return Vec::new();
    }

    let (base_bg, strong_bg) = match kind {
        LineKind::Add => (Some(palette.add_bg), Some(palette.add_strong_bg)),
        LineKind::Del => (Some(palette.del_bg), Some(palette.del_strong_bg)),
        LineKind::Context => (None, None),
    };
    let matches = match query {
        Some(q) if !q.is_empty() => match_ranges(text, q),
        _ => Vec::new(),
    };

    // Cut the line at every fg-span, segment and match boundary; each resulting
    // slice has a single (fg, bg) style.
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
    for (s, e) in &matches {
        bounds.insert(*s);
        bounds.insert(*e);
    }
    // Only slice on char boundaries: span/segment offsets come from this same
    // text so they align in practice, but filtering guarantees `text[a..b]` can
    // never panic on multibyte input (defense-in-depth, rule #1: no panics).
    let points: Vec<usize> = bounds
        .into_iter()
        .filter(|&p| text.is_char_boundary(p))
        .collect();

    let mut spans = Vec::with_capacity(points.len());
    for win in points.windows(2) {
        let (a, b) = (win[0], win[1]);
        if a >= b {
            continue;
        }
        let in_match = matches.iter().any(|&(s, e)| s <= a && a < e);

        let style = if in_match {
            // Search highlight wins over syntax/diff styling.
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
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
            style
        };
        spans.push(Span::styled(text[a..b].to_string(), style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::match_ranges;

    #[test]
    fn lowercase_query_is_case_insensitive() {
        // Smart case: an all-lowercase query matches both cases.
        assert_eq!(
            match_ranges("let Greeting = greeting;", "greeting"),
            vec![(4, 12), (15, 23)]
        );
    }

    #[test]
    fn uppercase_in_query_is_case_sensitive() {
        // An uppercase letter makes the search case-sensitive (smart case).
        assert_eq!(
            match_ranges("let Greeting = greeting;", "Greeting"),
            vec![(4, 12)]
        );
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(match_ranges("hello world", "xyz").is_empty());
        assert!(match_ranges("short", "longer than text").is_empty());
        assert!(match_ranges("anything", "").is_empty());
    }
}
