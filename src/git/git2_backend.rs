//! `git2`/libgit2 implementation of [`GitBackend`]. The only module (besides the
//! `testutil` fixture builder) allowed to depend on `git2`.
//!
//! Supports all concrete [`CompareSpec`] kinds (a `Pr` must be resolved to a
//! `Range` first — see `git::base`).

use super::{CompareSpec, FileBlobs, FileChange, GitBackend, RepoContext, Status};
use anyhow::{bail, Context, Result};
use git2::{Commit, Delta, DiffFindOptions, DiffOptions, Repository, Tree};
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

    /// Resolve a revspec to its tree.
    fn tree_of(&self, rev: &str) -> Result<Tree<'_>> {
        let obj = self
            .repo
            .revparse_single(rev)
            .with_context(|| format!("cannot resolve revision {rev}"))?;
        Ok(obj.peel_to_tree()?)
    }

    /// Resolve a revspec to its commit.
    fn commit_of(&self, rev: &str) -> Result<Commit<'_>> {
        let obj = self
            .repo
            .revparse_single(rev)
            .with_context(|| format!("cannot resolve revision {rev}"))?;
        Ok(obj.peel_to_commit()?)
    }

    /// Untracked-aware options for working-tree diffs.
    fn workdir_opts() -> DiffOptions {
        let mut o = DiffOptions::new();
        o.include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true);
        o
    }

    /// Build the libgit2 diff for a comparison, with rename/copy detection.
    fn diff_for(&self, spec: &CompareSpec) -> Result<git2::Diff<'_>> {
        let mut diff = match spec {
            CompareSpec::Uncommitted => {
                let head = self.head_tree()?;
                let mut o = Self::workdir_opts();
                self.repo
                    .diff_tree_to_workdir_with_index(head.as_ref(), Some(&mut o))?
            }
            CompareSpec::Staged => {
                let head = self.head_tree()?;
                let index = self.repo.index()?;
                self.repo
                    .diff_tree_to_index(head.as_ref(), Some(&index), None)?
            }
            CompareSpec::WorkdirVs(rev) => {
                let tree = self.tree_of(rev)?;
                let mut o = Self::workdir_opts();
                self.repo
                    .diff_tree_to_workdir_with_index(Some(&tree), Some(&mut o))?
            }
            CompareSpec::Range(a, b) => {
                let (ta, tb) = (self.tree_of(a)?, self.tree_of(b)?);
                self.repo.diff_tree_to_tree(Some(&ta), Some(&tb), None)?
            }
            CompareSpec::Commit(c) => {
                let commit = self.commit_of(c)?;
                let ctree = commit.tree()?;
                let parent = match commit.parent(0) {
                    Ok(p) => Some(p.tree()?),
                    Err(_) => None, // root commit
                };
                self.repo
                    .diff_tree_to_tree(parent.as_ref(), Some(&ctree), None)?
            }
            CompareSpec::Pr(_) => {
                bail!("PR comparisons must be resolved to a Range first (see git::base)")
            }
        };

        let mut find = DiffFindOptions::new();
        find.renames(true).copies(true);
        diff.find_similar(Some(&mut find))?;
        Ok(diff)
    }

    /// Read a path's blob from a tree, if present.
    fn blob_in_tree(&self, tree: &Tree, path: &Path) -> Result<Option<Vec<u8>>> {
        match tree.get_path(path) {
            Ok(entry) => Ok(Some(
                entry
                    .to_object(&self.repo)?
                    .peel_to_blob()?
                    .content()
                    .to_vec(),
            )),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Read a path's blob from the `HEAD` tree.
    fn blob_at_head(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        match self.head_tree()? {
            Some(tree) => self.blob_in_tree(&tree, path),
            None => Ok(None),
        }
    }

    /// Read a path's blob from the index (staged content).
    fn blob_in_index(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        let index = self.repo.index()?;
        match index.get_path(path, 0) {
            Some(entry) => Ok(Some(self.repo.find_blob(entry.id)?.content().to_vec())),
            None => Ok(None),
        }
    }

    /// Read a path from the working tree (`None` if it was removed since listing).
    fn read_workdir(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        let workdir = self
            .repo
            .workdir()
            .context("bare repositories have no working tree")?;
        let full = workdir.join(path);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow::Error::new(e).context(format!("read {}", full.display()))),
        }
    }
}

/// Map a libgit2 delta status to our [`Status`].
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
        let branch = self
            .repo
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string).ok());
        Ok(RepoContext { root, branch })
    }

    fn changed_files(&self, spec: &CompareSpec) -> Result<Vec<FileChange>> {
        let diff = self.diff_for(spec)?;

        let mut files = Vec::new();
        for (idx, delta) in diff.deltas().enumerate() {
            let Some(status) = map_status(delta.status()) else {
                continue;
            };
            let new_path = delta.new_file().path().map(Path::to_path_buf);
            let old_path = delta.old_file().path().map(Path::to_path_buf);
            let path = match (&new_path, &old_path) {
                (Some(p), _) => p.clone(),
                (None, Some(p)) => p.clone(),
                (None, None) => continue,
            };
            let is_binary = delta.new_file().is_binary() || delta.old_file().is_binary();
            let (additions, deletions) = match git2::Patch::from_diff(&diff, idx) {
                Ok(Some(patch)) => {
                    let (_ctx, add, del) = patch.line_stats().unwrap_or((0, 0, 0));
                    (add, del)
                }
                _ => (0, 0),
            };

            files.push(FileChange {
                path,
                old_path: if matches!(status, Status::Renamed | Status::Copied) {
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
        let old_path = change.old_path.as_deref().unwrap_or(&change.path);

        let old = if change.status == Status::Added {
            None
        } else {
            match spec {
                CompareSpec::Uncommitted | CompareSpec::Staged => self.blob_at_head(old_path)?,
                CompareSpec::WorkdirVs(rev) => self.blob_in_tree(&self.tree_of(rev)?, old_path)?,
                CompareSpec::Range(a, _) => self.blob_in_tree(&self.tree_of(a)?, old_path)?,
                CompareSpec::Commit(c) => match self.commit_of(c)?.parent(0) {
                    Ok(p) => self.blob_in_tree(&p.tree()?, old_path)?,
                    Err(_) => None,
                },
                CompareSpec::Pr(_) => bail!("PR must be resolved before reading contents"),
            }
        };

        let new = if change.status == Status::Deleted {
            None
        } else {
            match spec {
                CompareSpec::Uncommitted | CompareSpec::WorkdirVs(_) => {
                    self.read_workdir(&change.path)?
                }
                CompareSpec::Staged => self.blob_in_index(&change.path)?,
                CompareSpec::Range(_, b) => self.blob_in_tree(&self.tree_of(b)?, &change.path)?,
                CompareSpec::Commit(c) => {
                    self.blob_in_tree(&self.commit_of(c)?.tree()?, &change.path)?
                }
                CompareSpec::Pr(_) => bail!("PR must be resolved before reading contents"),
            }
        };

        Ok((old, new))
    }

    fn detect_base(&self, candidates: &[String]) -> Result<Option<String>> {
        let Some(head_oid) = self.repo.head().ok().and_then(|h| h.target()) else {
            return Ok(None);
        };
        let current = self
            .repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(str::to_string).ok());

        for name in candidates {
            if current.as_deref() == Some(name.as_str()) {
                continue;
            }
            if let Ok(commit) = self.commit_of(name) {
                if let Ok(base) = self.repo.merge_base(head_oid, commit.id()) {
                    if base != head_oid {
                        return Ok(Some(base.to_string()));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// Open the backend for the repository at or above `path`.
pub fn open(path: &Path) -> Result<Git2Backend> {
    Git2Backend::open(path)
}
