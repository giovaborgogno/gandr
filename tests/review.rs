//! Unit tests for review state: transitions, changed-since, persistence, hashing.

use gandr::diff::{FileDiff, Hunk, Line, LineKind};
use gandr::git::{FileChange, Status};
use gandr::review::{diff_hash, ReviewState, ReviewStatus};
use std::path::Path;

fn file_diff(path: &str, text: &str) -> FileDiff {
    FileDiff {
        change: FileChange {
            path: path.into(),
            old_path: None,
            status: Status::Modified,
            is_binary: false,
            additions: 1,
            deletions: 0,
        },
        hunks: vec![Hunk {
            old_start: 1,
            new_start: 1,
            header: "@@".into(),
            lines: vec![Line {
                kind: LineKind::Add,
                old_no: None,
                new_no: Some(1),
                text: text.into(),
                segments: vec![],
            }],
        }],
        old_text: String::new(),
        new_text: text.into(),
        image: None,
    }
}

#[test]
fn toggle_transitions() {
    let mut st = ReviewState::default();
    let p = Path::new("a.rs");
    assert_eq!(st.status("k", p, 1), ReviewStatus::Unreviewed);

    st.toggle("k", p, 1);
    assert_eq!(st.status("k", p, 1), ReviewStatus::Reviewed);

    // The content hash changed → flagged as changed-since-reviewed.
    assert_eq!(st.status("k", p, 2), ReviewStatus::ChangedSinceReviewed);

    // Acknowledge the change (toggle with the new hash) → reviewed again.
    st.toggle("k", p, 2);
    assert_eq!(st.status("k", p, 2), ReviewStatus::Reviewed);

    // Toggle with the matching hash → unreviewed.
    st.toggle("k", p, 2);
    assert_eq!(st.status("k", p, 2), ReviewStatus::Unreviewed);
}

#[test]
fn keys_are_independent() {
    let mut st = ReviewState::default();
    let p = Path::new("a.rs");
    st.toggle("uncommitted", p, 1);
    assert_eq!(st.status("uncommitted", p, 1), ReviewStatus::Reviewed);
    // A different comparison key doesn't see it.
    assert_eq!(st.status("staged", p, 1), ReviewStatus::Unreviewed);
}

#[test]
fn persistence_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gandr").join("state.json");

    let mut st = ReviewState::default();
    st.toggle("k", Path::new("a.rs"), 42);
    st.save(&path).unwrap();

    let loaded = ReviewState::load(&path);
    assert_eq!(
        loaded.status("k", Path::new("a.rs"), 42),
        ReviewStatus::Reviewed
    );
    assert_eq!(
        loaded.status("k", Path::new("a.rs"), 99),
        ReviewStatus::ChangedSinceReviewed
    );
}

#[test]
fn load_missing_file_is_empty() {
    let st = ReviewState::load(Path::new("/nonexistent/gandr/state.json"));
    assert_eq!(
        st.status("k", Path::new("a.rs"), 1),
        ReviewStatus::Unreviewed
    );
}

#[test]
fn diff_hash_changes_with_content() {
    let a = file_diff("a.rs", "let x = 1;");
    let b = file_diff("a.rs", "let x = 2;");
    let same = file_diff("a.rs", "let x = 1;");
    assert_ne!(diff_hash(&a), diff_hash(&b));
    assert_eq!(diff_hash(&a), diff_hash(&same));
}

#[test]
fn reviewed_count_counts_only_fresh() {
    let mut st = ReviewState::default();
    let files = vec![file_diff("a.rs", "x"), file_diff("b.rs", "y")];
    st.toggle("k", Path::new("a.rs"), diff_hash(&files[0]));
    assert_eq!(st.reviewed_count("k", &files), 1);
}
