//! Syntax highlighting (syntect + two-face) and the diff color palette.
//!
//! Syntax sets and the theme are loaded once (process-wide) and shared. A
//! [`Highlighter`] resolves a file's syntax once and highlights lines on demand;
//! [`compose`] layers syntax foreground over the diff background, with a stronger
//! background on word-level changed segments. Theme auto-detection lands in M7.

pub mod compose;

use ratatui::style::Color;
use std::path::Path;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use two_face::theme::EmbeddedThemeName;

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

fn syntaxes() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(two_face::syntax::extra_no_newlines)
}

fn theme() -> &'static Theme {
    THEME.get_or_init(|| {
        two_face::theme::extra()
            .get(EmbeddedThemeName::Base16OceanDark)
            .clone()
    })
}

/// A foreground color spanning a byte range `[start, end)` of a line.
#[derive(Debug, Clone, Copy)]
pub struct FgSpan {
    pub start: usize,
    pub end: usize,
    pub color: Color,
}

/// Per-file syntax highlighter: resolves the syntax once, highlights lines on demand.
pub struct Highlighter {
    syntax: &'static SyntaxReference,
}

impl Highlighter {
    /// Resolve the syntax for a path by extension, falling back to plain text.
    pub fn for_path(path: &Path) -> Self {
        let ss = syntaxes();
        let syntax = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|ext| ss.find_syntax_by_extension(ext))
            .unwrap_or_else(|| ss.find_syntax_plain_text());
        Self { syntax }
    }

    /// Foreground spans for one line. Highlighting is per-line (state is not
    /// carried across lines), so multi-line constructs aren't tracked yet — good
    /// enough for M3; empty on any highlighter error.
    pub fn fg_spans(&self, text: &str) -> Vec<FgSpan> {
        let mut hl = HighlightLines::new(self.syntax, theme());
        let Ok(ranges) = hl.highlight_line(text, syntaxes()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(ranges.len());
        let mut idx = 0;
        for (style, piece) in ranges {
            let start = idx;
            let end = idx + piece.len();
            out.push(FgSpan {
                start,
                end,
                color: to_color(style.foreground),
            });
            idx = end;
        }
        out
    }
}

fn to_color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Diff background palette. Fixed dark for M3; light/dark auto-detection (theme
/// = "auto", OSC 11) lands in M7.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub add_bg: Color,
    pub add_strong_bg: Color,
    pub del_bg: Color,
    pub del_strong_bg: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            add_bg: Color::Rgb(20, 48, 28),
            add_strong_bg: Color::Rgb(36, 94, 52),
            del_bg: Color::Rgb(58, 26, 28),
            del_strong_bg: Color::Rgb(110, 40, 44),
        }
    }
}
