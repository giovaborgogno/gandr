//! `git2`/libgit2 implementation of [`GitBackend`]. The only module (besides the
//! `testutil` fixture builder) allowed to depend on `git2`.
//!
//! M1 implements [`CompareSpec::Uncommitted`] (all uncommitted changes vs `HEAD`);
//! the remaining comparison kinds land in M5.

use super::{CompareSpec, FileBlobs, FileChange, GitBackend, RepoContext, Status};
use anyhow::{bail, Context, Result};
use git2::{Delta, DiffFindOptions, DiffOptions, Repository, Tree};
use std::path::Path;

/// A backend bound to one repository.
pub struct Git2Backend {
    repo: Repository,
}

impl Git2Backend {
    /// Open the repository containing `path` (searches upward like git does).
    pub fn open(path: &Path) -> Result<Self> {
        let repo = Repository::discover(path)
            .with_context(|| format!("no git repository at {}", path.display()))?;
        Ok(Self { repo })
    }

    /// The `HEAD` tree, or `None` if `HEAD` is unborn (no commits yet).
    fn head_tree(&self) -> Result<Option<Tree<'_>>> {
        match self.repo.head() {
            Ok(head) => Ok(Some(head.peel_to_tree()?)),
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Build the `HEAD`-tree → working-tree (incl. index) diff with rename detection.
    fn uncommitted_diff(&self) -> Result<git2::Diff<'_>> {
        let head = self.head_tree()?;
        let mut opts = DiffOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true);

        let mut diff = self
            .repo
            .diff_tree_to_workdir_with_index(head.as_ref(), Some(&mut opts))?;

        let mut find = DiffFindOptions::new();
        find.renames(true).copies(true);
        diff.find_similar(Some(&mut find))?;
        Ok(diff)
    }
}

/// Map a libgit2 delta status to our [`Status`]. Returns `None` for kinds we don't
/// surface (unmodified, ignored, type-change, conflicted).
fn map_status(delta: Delta) -> Option<Status> {
    match delta {
        Delta::Added | Delta::Untracked => Some(Status::Added),
        Delta::Modified => Some(Status::Modified),
        Delta::Deleted => Some(Status::Deleted),
        Delta::Renamed => Some(Status::Renamed),
        Delta::Copied => Some(Status::Copied),
        _ => None,
    }
}

impl GitBackend for Git2Backend {
    fn context(&self) -> Result<RepoContext> {
        let root = self
            .repo
            .workdir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.repo.path().to_path_buf());

        // None when HEAD is unborn or detached.
        let branch = self
            .repo
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string).ok());
        Ok(RepoContext { root, branch })
    }

    fn changed_files(&self, spec: &CompareSpec) -> Result<Vec<FileChange>> {
        let diff = match spec {
            CompareSpec::Uncommitted => self.uncommitted_diff()?,
            other => bail!("comparison {other:?} not implemented yet (lands in M5)"),
        };

        let mut files = Vec::new();
        for (idx, delta) in diff.deltas().enumerate() {
            let Some(status) = map_status(delta.status()) else {
                continue;
            };

            let new_path = delta.new_file().path().map(Path::to_path_buf);
            let old_path = delta.old_file().path().map(Path::to_path_buf);

            // Primary path: the new path, except for deletions where only old exists.
            let path = match (&new_path, &old_path) {
                (Some(p), _) => p.clone(),
                (None, Some(p)) => p.clone(),
                (None, None) => continue,
            };

            let is_binary = delta.new_file().is_binary() || delta.old_file().is_binary();

            // Per-file line counts from libgit2 (cheap; our engine recomputes hunks).
            let (additions, deletions) = match git2::Patch::from_diff(&diff, idx) {
                Ok(Some(patch)) => {
                    let (_ctx, add, del) = patch.line_stats().unwrap_or((0, 0, 0));
                    (add, del)
                }
                _ => (0, 0),
            };

            files.push(FileChange {
                path,
                old_path: if status == Status::Renamed || status == Status::Copied {
                    old_path
                } else {
                    None
                },
                status,
                is_binary,
                additions,
                deletions,
            });
        }

        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    fn file_contents(&self, spec: &CompareSpec, change: &FileChange) -> Result<FileBlobs> {
        match spec {
            CompareSpec::Uncommitted => {}
            other => bail!("comparison {other:?} not implemented yet (lands in M5)"),
        }

        // Old side: the file as it is in HEAD (None if added or HEAD unborn).
        let old = if change.status == Status::Added {
            None
        } else {
            let old_path = change.old_path.as_deref().unwrap_or(&change.path);
            self.blob_at_head(old_path)?
        };

        // New side: the file in the working tree (None if deleted).
        let new = if change.status == Status::Deleted {
            None
        } else {
            let workdir = self
                .repo
                .workdir()
                .context("bare repositories have no working tree")?;
            let full = workdir.join(&change.path);
            match std::fs::read(&full) {
                Ok(bytes) => Some(bytes),
                // The file was removed since changed_files() ran (e.g. an agent
                // editing in a watched working tree). Treat it as absent rather
                // than aborting the whole diff.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => {
                    return Err(anyhow::Error::new(e).context(format!("read {}", full.display())))
                }
            }
        };

        Ok((old, new))
    }
}

impl Git2Backend {
    /// Read a path's blob from the `HEAD` tree, if present.
    fn blob_at_head(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        let Some(tree) = self.head_tree()? else {
            return Ok(None);
        };
        match tree.get_path(path) {
            Ok(entry) => {
                let blob = entry.to_object(&self.repo)?.peel_to_blob()?;
                Ok(Some(blob.content().to_vec()))
            }
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// Open the backend for the repository at or above `path`. Convenience for callers
/// that only know the trait by name.
pub fn open(path: &Path) -> Result<Git2Backend> {
    Git2Backend::open(path)
}
