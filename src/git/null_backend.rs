//! A no-op [`GitBackend`] for directories that aren't git repositories.
//!
//! gandr then opens in "files-only" mode: the Repo tab (browse / preview /
//! search the filesystem) works exactly as it does in a repo, and the Diff tab
//! is simply empty — there's nothing to compare. This lets gandr double as a
//! fast read-only file browser, not just a git reviewer.

use crate::git::{CompareSpec, FileBlobs, FileChange, GitBackend, RefEntry, RepoContext};
use anyhow::Result;
use std::path::PathBuf;

pub struct NullBackend {
    root: PathBuf,
}

impl NullBackend {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl GitBackend for NullBackend {
    fn context(&self) -> Result<RepoContext> {
        Ok(RepoContext {
            root: self.root.clone(),
            // Never written to in files-only mode (no review state without git).
            git_dir: self.root.join(".git"),
            branch: None,
        })
    }

    fn changed_files(&self, _spec: &CompareSpec) -> Result<Vec<FileChange>> {
        Ok(Vec::new())
    }

    fn file_contents(&self, _spec: &CompareSpec, _change: &FileChange) -> Result<FileBlobs> {
        Ok((None, None))
    }

    fn detect_base(&self, _candidates: &[String]) -> Result<Option<String>> {
        Ok(None)
    }

    fn list_refs(&self) -> Result<Vec<RefEntry>> {
        Ok(Vec::new())
    }
}
