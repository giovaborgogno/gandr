//! Test fixtures: deterministic temporary git repos with known changes.
//!
//! Used by tests and examples (never by the binary's real code path). Build a
//! repo, commit a baseline, then introduce the change you want to diff. See
//! `docs/testing.md` and the `/new-fixture` skill.
//!
//! This is the one place outside `src/git/` that touches `git2` directly: it
//! *constructs* repos rather than reading diffs, so it's exempt from the
//! `GitBackend` layering rule (and from `tests/architecture.rs`).

use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A throwaway git repository in a temp dir, cleaned up on drop.
pub struct Fixture {
    dir: TempDir,
    repo: git2::Repository,
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixture {
    /// Create an empty repo with a deterministic identity and branch configured.
    /// The initial branch is fixed to `main` so snapshots don't depend on the
    /// host's `init.defaultBranch`.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &opts).expect("git init");
        {
            let mut cfg = repo.config().expect("repo config");
            cfg.set_str("user.name", "gandr fixtures")
                .expect("set name");
            cfg.set_str("user.email", "fixtures@gandr.test")
                .expect("set email");
        }
        Self { dir, repo }
    }

    /// Repository root path.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Write (create or overwrite) a file, creating parent dirs. Does not stage.
    pub fn write(&self, rel: &str, contents: &str) -> &Self {
        let path = self.dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&path, contents).expect("write file");
        self
    }

    /// Write raw bytes (e.g. for binary fixtures). Does not stage.
    pub fn write_bytes(&self, rel: &str, contents: &[u8]) -> &Self {
        let path = self.dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&path, contents).expect("write file");
        self
    }

    /// Delete a file from the working tree.
    pub fn remove(&self, rel: &str) -> &Self {
        std::fs::remove_file(self.dir.path().join(rel)).expect("remove file");
        self
    }

    /// Stage everything (including deletions) and commit. Returns the commit id.
    pub fn commit(&self, message: &str) -> git2::Oid {
        let mut index = self.repo.index().expect("open index");
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .expect("add all");
        index.update_all(["*"], None).expect("update index"); // pick up deletions
        index.write().expect("write index");

        let tree_id = index.write_tree().expect("write tree");
        let tree = self.repo.find_tree(tree_id).expect("find tree");
        let sig = self.repo.signature().expect("signature");

        let parents = match self.repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
            Some(parent) => vec![parent],
            None => vec![],
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

        self.repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
            .expect("commit")
    }

    /// Stage all current changes into the index (without committing), so they
    /// show up as "staged" in an index-vs-HEAD comparison.
    pub fn stage_all(&self) -> &Self {
        let mut index = self.repo.index().expect("open index");
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .expect("add all");
        index.update_all(["*"], None).expect("update index");
        index.write().expect("write index");
        self
    }

    /// Absolute path to a file in the repo.
    pub fn file(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// Create a branch at the current `HEAD` and check it out.
    pub fn checkout_new_branch(&self, name: &str) -> &Self {
        let head = self
            .repo
            .head()
            .expect("head")
            .peel_to_commit()
            .expect("head commit");
        self.repo.branch(name, &head, false).expect("create branch");
        self.repo
            .set_head(&format!("refs/heads/{name}"))
            .expect("set head");
        self
    }

    /// Create a lightweight tag at the current `HEAD`.
    pub fn tag(&self, name: &str) -> &Self {
        let head = self
            .repo
            .head()
            .expect("head")
            .peel_to_commit()
            .expect("head commit");
        self.repo
            .tag_lightweight(name, head.as_object(), false)
            .expect("create tag");
        self
    }
}
