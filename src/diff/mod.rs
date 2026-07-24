//! Diff model produced by the engine (imara-diff) from raw file contents.
//!
//! The engine itself (M1) computes line-level hunks and intra-line word
//! [`Segment`]s; this module just defines the shapes the UI renders.

use crate::git::FileChange;
use crate::image_preview::ImageInfo;

pub mod engine;
pub mod fold;
pub mod word;

/// Whether a line was added, removed, or is unchanged context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Del,
}

/// A byte range within a [`Line::text`] and whether it changed (word-level).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub changed: bool,
}

/// A single rendered line of a diff.
#[derive(Debug, Clone)]
pub struct Line {
    pub kind: LineKind,
    /// Line number on the old side, if present.
    pub old_no: Option<u32>,
    /// Line number on the new side, if present.
    pub new_no: Option<u32>,
    /// Line text, without trailing newline.
    pub text: String,
    /// Word-level segments (empty until M3 / when word-diff is off).
    pub segments: Vec<Segment>,
}

/// A contiguous block of changes plus surrounding context.
#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: u32,
    pub new_start: u32,
    pub header: String,
    pub lines: Vec<Line>,
}

/// The full diff for one file.
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub change: FileChange,
    pub hunks: Vec<Hunk>,
    /// Full decoded old/new file text (empty for an absent side or a binary
    /// file). Kept so the UI can highlight each side with carried state (M12)
    /// and reveal folded context on demand (per-gap expand). See
    /// [`engine::split_lines`] for the line indexing that matches `old_no`/`new_no`.
    pub old_text: String,
    pub new_text: String,
    /// Raster-image metadata for a binary image file (M15), probed from the
    /// current side's bytes while building the diff. `None` for non-images and
    /// non-binary files. Feeds the preview placeholder now and the rendered
    /// image (M15b).
    pub image: Option<ImageInfo>,
}
