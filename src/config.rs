//! Configuration. For M0 this is just in-memory defaults; loading from
//! `~/.config/gdiff/config.toml` + per-repo `.gdiff.toml` lands in M7.

/// How the diff is laid out in the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Single column, `-`/`+` lines interleaved (GitHub "unified").
    Unified,
    /// Two columns, old on the left, new on the right.
    SideBySide,
}

impl ViewMode {
    /// Toggle between the two layouts.
    pub fn toggled(self) -> Self {
        match self {
            ViewMode::Unified => ViewMode::SideBySide,
            ViewMode::SideBySide => ViewMode::Unified,
        }
    }
}

/// Runtime configuration. Field set grows with the milestones; see `DESIGN.md` §8.
#[derive(Debug, Clone)]
pub struct Config {
    pub default_view: ViewMode,
    pub word_diff: bool,
    pub auto_refresh: bool,
    /// Smart comparison auto-selection is opt-in (see DESIGN.md §2).
    pub smart_compare: bool,
    pub context_lines: usize,
    pub tab_width: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_view: ViewMode::Unified,
            word_diff: true,
            auto_refresh: true,
            smart_compare: false,
            context_lines: 3,
            tab_width: 4,
        }
    }
}
