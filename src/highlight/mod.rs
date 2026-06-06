//! Syntax highlighting (syntect + two-face) and style composition.
//!
//! Wired in M3: syntect highlights each line once (cached per file), then
//! `compose` overlays syntax foreground + diff background + word-level background
//! into ratatui `Span`s. Empty for now so the module boundary exists from M0.
