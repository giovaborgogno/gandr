//! Unit tests for the file-tree builder (compaction, collapse, ordering).

use gdiff::diff::FileDiff;
use gdiff::git::{FileChange, Status};
use gdiff::ui::tree::{build_rows, RowKind};
use std::collections::HashSet;
use std::path::PathBuf;

fn file(path: &str) -> FileDiff {
    FileDiff {
        change: FileChange {
            path: PathBuf::from(path),
            old_path: None,
            status: Status::Modified,
            is_binary: false,
            additions: 0,
            deletions: 0,
        },
        hunks: vec![],
        old_text: String::new(),
        new_text: String::new(),
    }
}

fn labels(rows: &[gdiff::ui::tree::Row]) -> Vec<String> {
    rows.iter().map(|r| r.label.clone()).collect()
}

#[test]
fn compacts_single_child_directory_chains() {
    let files = vec![file("src/app/mod.rs"), file("README.md")];
    let rows = build_rows(&files, &HashSet::new());
    // `src` → `app` is a single-child chain, compacted to one node.
    assert_eq!(labels(&rows), vec!["src/app", "mod.rs", "README.md"]);
}

#[test]
fn does_not_compact_dir_with_multiple_children() {
    let files = vec![file("src/a.rs"), file("src/b.rs")];
    let rows = build_rows(&files, &HashSet::new());
    assert_eq!(labels(&rows), vec!["src", "a.rs", "b.rs"]);
}

#[test]
fn collapsed_dir_hides_children() {
    let files = vec![file("src/a.rs"), file("README.md")];
    let mut collapsed = HashSet::new();
    collapsed.insert(PathBuf::from("src"));

    let rows = build_rows(&files, &collapsed);
    assert_eq!(labels(&rows), vec!["src", "README.md"]); // a.rs is hidden
    match &rows[0].kind {
        RowKind::Dir { expanded, .. } => assert!(!expanded),
        other => panic!("expected collapsed dir, got {other:?}"),
    }
}

#[test]
fn file_rows_carry_their_index() {
    let files = vec![file("a.rs"), file("b.rs")];
    let rows = build_rows(&files, &HashSet::new());
    let indices: Vec<_> = rows.iter().filter_map(|r| r.file_index()).collect();
    assert_eq!(indices, vec![0, 1]);
}
