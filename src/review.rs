//! Per-repo review tracking: which files were marked reviewed, plus a content
//! hash so files that changed *after* review can be flagged. Persisted to
//! `.git/gdiff/state.json`, keyed by comparison.

use crate::diff::{FileDiff, LineKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// FNV-1a over a byte slice. A fixed, platform-stable hash — unlike
/// `DefaultHasher` (SipHash with an unspecified, version-dependent result) — so
/// review hashes persisted in `state.json` stay valid across toolchain upgrades
/// and machines.
fn fnv1a(state: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *state ^= b as u64;
        *state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// Review state of a single file in the current comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStatus {
    Unreviewed,
    Reviewed,
    /// Reviewed earlier, but the diff changed since (e.g. the agent edited it again).
    ChangedSinceReviewed,
}

/// Persisted review state: comparison key → (file path → content hash at review).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReviewState {
    #[serde(default)]
    reviewed: HashMap<String, HashMap<String, u64>>,
}

/// A stable hash of a file's diff content (path + each line's kind and text), used
/// to detect whether a reviewed file changed afterwards.
pub fn diff_hash(file: &FileDiff) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset basis
    fnv1a(&mut h, file.change.path.to_string_lossy().as_bytes());
    for hunk in &file.hunks {
        for line in &hunk.lines {
            let kind: u8 = match line.kind {
                LineKind::Context => 0,
                LineKind::Add => 1,
                LineKind::Del => 2,
            };
            fnv1a(&mut h, &[kind]);
            fnv1a(&mut h, line.text.as_bytes());
        }
    }
    h
}

impl ReviewState {
    /// Where the state file lives for a repo at `repo_root`.
    pub fn state_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".git").join("gdiff").join("state.json")
    }

    /// Load from disk, returning an empty state if missing or unreadable.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist to disk (creating parent directories).
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Status of one file under a comparison, given its current content hash.
    pub fn status(&self, key: &str, path: &Path, current_hash: u64) -> ReviewStatus {
        let stored = self
            .reviewed
            .get(key)
            .and_then(|m| m.get(path.to_string_lossy().as_ref()));
        match stored {
            Some(&h) if h == current_hash => ReviewStatus::Reviewed,
            Some(_) => ReviewStatus::ChangedSinceReviewed,
            None => ReviewStatus::Unreviewed,
        }
    }

    /// Toggle review for a file: unreviewed → reviewed; reviewed(fresh) →
    /// unreviewed; changed-since → reviewed again (acknowledge the change).
    pub fn toggle(&mut self, key: &str, path: &Path, current_hash: u64) {
        let entry = self.reviewed.entry(key.to_string()).or_default();
        let p = path.to_string_lossy().into_owned();
        match entry.get(&p) {
            Some(&h) if h == current_hash => {
                entry.remove(&p);
            }
            _ => {
                entry.insert(p, current_hash);
            }
        }
    }

    /// Count of fresh-reviewed files among `files` under `key`.
    pub fn reviewed_count(&self, key: &str, files: &[FileDiff]) -> usize {
        files
            .iter()
            .filter(|f| self.status(key, &f.change.path, diff_hash(f)) == ReviewStatus::Reviewed)
            .count()
    }
}
