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

#[test]
fn content_highlighting_is_multiline_aware() {
    // A single .rs file with a block comment spanning several lines (M12).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("a.rs"),
        "/* open comment\ninterior text line\nclose */\nlet code = 1;\n",
    )
    .unwrap();

    let mut browser = Browser::new(root.to_path_buf());
    // Cursor starts on the only file, which loads automatically (highlights are
    // produced off-thread; drive that job by hand here).
    assert!(
        browser.loaded().unwrap().highlights.is_empty(),
        "plain until highlighted"
    );
    warm(&mut browser);

    let loaded = browser.loaded().expect("file loaded");
    assert_eq!(loaded.path.file_name().unwrap(), "a.rs");
    // One highlight entry per line, computed with carried state.
    assert_eq!(loaded.highlights.len(), loaded.lines.len());

    // The interior comment line carries the opener's comment color — only
    // possible because state is carried across lines (stateless per-line
    // highlighting would treat it as plain code).
    let opener = loaded.highlights[0].first().map(|s| s.color);
    let interior = loaded.highlights[1].first().map(|s| s.color);
    assert!(opener.is_some());
    assert_eq!(
        interior, opener,
        "interior of block comment should be comment-colored"
    );

    // Changing theme drops the highlights so they're recomputed for the new mode.
    browser.set_mode(gdiff::highlight::ThemeMode::Light);
    assert!(browser.loaded().unwrap().highlights.is_empty());
    warm(&mut browser);
    assert_eq!(
        browser.loaded().unwrap().highlights.len(),
        browser.loaded().unwrap().lines.len()
    );
}

/// Drive the browser's async highlight to completion (the app does this via a
/// background job + `apply_highlights`).
fn warm(browser: &mut Browser) {
    if let Some((path, lines)) = browser.highlight_target() {
        let spans =
            gdiff::highlight::Highlighter::for_path(&path, browser.mode()).highlight_file(&lines);
        browser.apply_highlights(&path, spans);
    }
}
