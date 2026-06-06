//! Git backend abstraction.
//!
//! Everything above this module depends only on the [`GitBackend`] trait and the
//! DTOs here — never on `git2::*`. This is the single seam that lets us swap the
//! implementation (e.g. to `gix`) later without touching diff/UI code (ADR 0001).

use anyhow::Result;
use std::path::PathBuf;

/// What to compare. The bare `gdiff` default is [`CompareSpec::Uncommitted`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareSpec {
    /// All uncommitted changes (staged + unstaged) vs `HEAD`.
    Uncommitted,
    /// Staged changes only (index vs `HEAD`).
    Staged,
    /// Working tree vs an arbitrary ref.
    WorkdirVs(String),
    /// A commit range `a..b`.
    Range(String, String),
    /// The changes introduced by a single commit.
    Commit(String),
    /// A pull request (current branch's PR if `None`), resolved via `gh`.
    Pr(Option<u32>),
}

impl CompareSpec {
    /// Whether this comparison can change underneath us (so it's worth watching).
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            CompareSpec::Uncommitted | CompareSpec::Staged | CompareSpec::WorkdirVs(_)
        )
    }
}

/// Per-file change status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
}

impl Status {
    /// Single-letter marker shown in the file tree (`M`/`A`/`D`/`R`/`C`).
    pub fn marker(self) -> char {
        match self {
            Status::Added => 'A',
            Status::Modified => 'M',
            Status::Deleted => 'D',
            Status::Renamed => 'R',
            Status::Copied => 'C',
        }
    }
}

/// One changed file in a comparison.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: PathBuf,
    /// Previous path for renames/copies.
    pub old_path: Option<PathBuf>,
    pub status: Status,
    pub is_binary: bool,
    pub additions: usize,
    pub deletions: usize,
}

/// Repo-level context for the header line.
#[derive(Debug, Clone)]
pub struct RepoContext {
    pub root: PathBuf,
    pub branch: Option<String>,
}

/// Old and new contents of a file; `None` on a side means the file is absent there
/// (added → old `None`; deleted → new `None`).
pub type FileBlobs = (Option<Vec<u8>>, Option<Vec<u8>>);

/// The only interface the rest of the app has to version control.
pub trait GitBackend {
    /// Repo root + current branch, for the header.
    fn context(&self) -> Result<RepoContext>;

    /// The list of changed files for a comparison.
    fn changed_files(&self, spec: &CompareSpec) -> Result<Vec<FileChange>>;

    /// Old and new contents for one file.
    fn file_contents(&self, spec: &CompareSpec, change: &FileChange) -> Result<FileBlobs>;
}
