//! Git backend abstraction.
//!
//! Everything above this module depends only on the [`GitBackend`] trait and the
//! DTOs here — never on `git2::*`. This is the single seam that lets us swap the
//! implementation (e.g. to `gix`) later without touching diff/UI code (ADR 0001).

use anyhow::Result;
use std::path::PathBuf;

pub mod base;
pub mod git2_backend;

/// What to compare. The bare `gandr` default is [`CompareSpec::Uncommitted`].
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

    /// Short human label for the header line.
    pub fn label(&self) -> String {
        match self {
            CompareSpec::Uncommitted => "uncommitted".to_string(),
            CompareSpec::Staged => "staged".to_string(),
            CompareSpec::WorkdirVs(r) => format!("vs {r}"),
            CompareSpec::Range(a, b) => format!("{a}..{b}"),
            CompareSpec::Commit(c) => format!("commit {c}"),
            CompareSpec::Pr(Some(n)) => format!("PR #{n}"),
            CompareSpec::Pr(None) => "PR".to_string(),
        }
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
    /// Working-tree root.
    pub root: PathBuf,
    /// The repository's git directory (libgit2's resolved path — the real one
    /// for worktrees/submodules, where `<root>/.git` is a file, not a dir). This
    /// is where gandr persists its review state.
    pub git_dir: PathBuf,
    pub branch: Option<String>,
}

/// What a [`RefEntry`] points at, for grouping/ordering in the ref picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
}

impl RefKind {
    /// Short tag shown next to the name in the picker.
    pub fn label(self) -> &'static str {
        match self {
            RefKind::LocalBranch => "branch",
            RefKind::RemoteBranch => "remote",
            RefKind::Tag => "tag",
        }
    }
}

/// A selectable git ref (branch/tag) for the compare-against picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEntry {
    /// The name used as a revision (e.g. `main`, `origin/main`, `v1.2.0`).
    pub name: String,
    pub kind: RefKind,
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

    /// Detect a base for the current branch: the merge-base SHA between `HEAD`
    /// and the first of `candidates` that exists and differs from `HEAD`.
    /// `None` if no suitable base is found.
    fn detect_base(&self, candidates: &[String]) -> Result<Option<String>>;

    /// All branches (local + remote) and tags, for the compare-against picker.
    /// Order: local branches, then remote branches, then tags (each alpha).
    fn list_refs(&self) -> Result<Vec<RefEntry>>;
}
