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
pub fn detect_mode() -> ThemeMode {
    match termbg::theme(std::time::Duration::from_millis(100)) {
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

    /// Foreground spans for one line. Highlighting is per-line (state is not
    /// carried across lines), so multi-line constructs aren't tracked yet — good
    /// enough for M3; empty on any highlighter error.
    pub fn fg_spans(&self, text: &str) -> Vec<FgSpan> {
        let mut hl = HighlightLines::new(self.syntax, theme(self.mode));
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
