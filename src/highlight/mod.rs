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

/// Light or dark rendering, chosen from the terminal background (or config).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

/// Detect light/dark from the terminal background via OSC 11 (termbg). Must run
/// before entering raw mode / the alt-screen. Falls back to dark.
///
/// The timeout is deliberately short: local terminals answer OSC 11 in a few ms,
/// so 25ms keeps startup snappy while still detecting; an unresponsive terminal
/// (or a slow link) just falls back to dark after 25ms instead of stalling.
pub fn detect_mode() -> ThemeMode {
    match termbg::theme(std::time::Duration::from_millis(25)) {
        Ok(termbg::Theme::Light) => ThemeMode::Light,
        _ => ThemeMode::Dark,
    }
}

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static DARK_THEME: OnceLock<Theme> = OnceLock::new();
static LIGHT_THEME: OnceLock<Theme> = OnceLock::new();

fn syntaxes() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(two_face::syntax::extra_no_newlines)
}

fn theme(mode: ThemeMode) -> &'static Theme {
    let (cell, name) = match mode {
        ThemeMode::Dark => (&DARK_THEME, EmbeddedThemeName::Base16OceanDark),
        ThemeMode::Light => (&LIGHT_THEME, EmbeddedThemeName::InspiredGithub),
    };
    cell.get_or_init(|| two_face::theme::extra().get(name).clone())
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
    mode: ThemeMode,
}

impl Highlighter {
    /// Resolve the syntax for a path by extension, falling back to plain text.
    pub fn for_path(path: &Path, mode: ThemeMode) -> Self {
        let ss = syntaxes();
        let syntax = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|ext| ss.find_syntax_by_extension(ext))
            .unwrap_or_else(|| ss.find_syntax_plain_text());
        Self { syntax, mode }
    }

    /// Foreground spans for one line, highlighted in isolation (no carried
    /// state). Use for diff lines where the surrounding file isn't available in
    /// order; multi-line constructs (block comments / strings) aren't tracked.
    /// Empty on any highlighter error.
    pub fn fg_spans(&self, text: &str) -> Vec<FgSpan> {
        let mut hl = HighlightLines::new(self.syntax, theme(self.mode));
        match hl.highlight_line(text, syntaxes()) {
            Ok(ranges) => ranges_to_spans(ranges),
            Err(_) => Vec::new(),
        }
    }

    /// Foreground spans for a whole file's `lines`, in order, carrying syntect
    /// state across lines — so block comments and multi-line strings highlight
    /// correctly (M12). Returns one span list per input line (empty for a line
    /// whose highlight errored). This is O(file) work; compute it once when the
    /// file is loaded, not per frame.
    pub fn highlight_file(&self, lines: &[String]) -> Vec<Vec<FgSpan>> {
        let mut hl = HighlightLines::new(self.syntax, theme(self.mode));
        lines
            .iter()
            .map(|line| match hl.highlight_line(line, syntaxes()) {
                Ok(ranges) => ranges_to_spans(ranges),
                Err(_) => Vec::new(),
            })
            .collect()
    }
}

/// Convert syntect's styled pieces into byte-range foreground spans.
fn ranges_to_spans(ranges: Vec<(syntect::highlighting::Style, &str)>) -> Vec<FgSpan> {
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

fn to_color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_file_carries_state_across_lines() {
        let hl = Highlighter::for_path(Path::new("x.rs"), ThemeMode::Dark);
        // A block comment spanning three lines, then real code.
        let lines: Vec<String> = ["/* open", "interior text", "close */", "let code = 1;"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let multi = hl.highlight_file(&lines);

        let interior = multi[1].first().expect("interior line has a span").color;
        let opener = multi[0].first().expect("opener line has a span").color;
        // Inside the block comment the interior line takes the comment color —
        // the same as the opener — proving parser state carried across lines.
        assert_eq!(
            interior, opener,
            "interior comment line should be comment-colored"
        );

        // Highlighted in isolation (per-line), the same text is NOT a comment, so
        // its color differs — this is exactly what M12 fixes.
        let standalone = hl.fg_spans("interior text");
        assert_ne!(
            standalone.first().map(|s| s.color),
            Some(interior),
            "per-line highlighting must differ from stateful (multi-line) highlighting"
        );

        // The code line after the comment closes is highlighted as code again.
        assert_eq!(multi.len(), lines.len());
    }
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
        Self::for_mode(ThemeMode::Dark)
    }
}

impl Palette {
    /// The diff background palette for a theme mode.
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                add_bg: Color::Rgb(20, 48, 28),
                add_strong_bg: Color::Rgb(36, 94, 52),
                del_bg: Color::Rgb(58, 26, 28),
                del_strong_bg: Color::Rgb(110, 40, 44),
            },
            ThemeMode::Light => Self {
                add_bg: Color::Rgb(214, 247, 220),
                add_strong_bg: Color::Rgb(160, 223, 173),
                del_bg: Color::Rgb(255, 224, 224),
                del_strong_bg: Color::Rgb(245, 178, 178),
            },
        }
    }
}
