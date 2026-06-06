//! Unit tests for the repo file browser (Files tab).

use gdiff::browser::{Browser, EntryKind};
use std::fs;

#[test]
fn lists_files_and_dirs_skipping_dot_git() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join(".git")).unwrap();
    fs::create_dir(root.join("target")).unwrap(); // git-ignored in real repos, but shown
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("README.md"), "x").unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();

    let browser = Browser::new(root.to_path_buf());
    let rows = browser.rows();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();

    // Directories first (alphabetical), then files; .git is skipped, target is shown.
    assert_eq!(names, vec!["src", "target", "README.md"]);
    assert!(!names.contains(&".git"));
    // src is a collapsed dir → its children aren't listed yet.
    assert!(!names.contains(&"main.rs"));
}

#[test]
fn expanding_a_dir_reveals_children() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn f() {}").unwrap();

    let mut browser = Browser::new(root.to_path_buf());
    // Cursor starts on `src` (first row, a dir).
    assert!(browser.cursor_is_dir());
    browser.toggle(); // expand src

    let rows = browser.rows();
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[0].kind, EntryKind::Dir { expanded: true }));
    assert_eq!(rows[1].name, "lib.rs");
}
