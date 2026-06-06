//! Repo-wide search via embedded crates — no external binaries.
//!
//! * **File-name search** (fd-style): walk the tree with `ignore` (respects
//!   `.gitignore`, skips `.git`/hidden) and smart-case substring-match names.
//! * **Content search** (ripgrep-style): the same walk fed through
//!   `grep-searcher` + `grep-regex` so the query is a smart-case regex.
//!
//! Both are pure functions (root + query → results) so they run on a background
//! thread (see `app::jobs`) and are unit-testable without a terminal.

use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::SearcherBuilder;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// Cap on results so a broad query on a huge repo stays bounded (memory + render).
pub const MAX_RESULTS: usize = 500;

/// Which kind of repo-wide search the Files tab is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Match file *names* (fd-style).
    Files,
    /// Match file *contents* (ripgrep-style).
    Content,
}

impl SearchMode {
    /// Flip between the two modes.
    pub fn toggled(self) -> Self {
        match self {
            SearchMode::Files => SearchMode::Content,
            SearchMode::Content => SearchMode::Files,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SearchMode::Files => "files",
            SearchMode::Content => "content",
        }
    }
}

/// One content-search hit: file (relative to root), 1-based line, line text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentMatch {
    pub path: PathBuf,
    pub line: u64,
    pub text: String,
}

/// Results of a repo-wide search, tagged by the mode that produced them.
#[derive(Debug, Clone)]
pub enum SearchResults {
    Files(Vec<PathBuf>),
    Content(Vec<ContentMatch>),
}

impl SearchResults {
    pub fn len(&self) -> usize {
        match self {
            SearchResults::Files(v) => v.len(),
            SearchResults::Content(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Run the search for `mode` under `root`, returning tagged results.
pub fn run(root: &Path, query: &str, mode: SearchMode) -> SearchResults {
    match mode {
        SearchMode::Files => SearchResults::Files(search_files(root, query)),
        SearchMode::Content => SearchResults::Content(search_content(root, query)),
    }
}

/// Smart-case substring matcher: case-insensitive unless the query has an
/// uppercase character (then case-sensitive), mirroring the in-diff search.
struct SmartCase {
    needle: String,
    sensitive: bool,
}

impl SmartCase {
    fn new(query: &str) -> Self {
        let sensitive = query.chars().any(|c| c.is_uppercase());
        let needle = if sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        SmartCase { needle, sensitive }
    }
    fn is_match(&self, hay: &str) -> bool {
        if self.sensitive {
            hay.contains(&self.needle)
        } else {
            hay.to_lowercase().contains(&self.needle)
        }
    }
}

/// A walk that respects `.gitignore` and skips hidden entries (incl. `.git`),
/// even when `root` isn't itself a git repo (`require_git(false)`) — so search
/// behaves the same whether gdiff is pointed at a repo or a subdirectory.
///
/// Note: this is intentionally stricter than the Files-tab browser, which lists
/// git-ignored and hidden files too. Search is ripgrep-style — skipping ignored
/// paths (e.g. `node_modules`, build output) keeps it fast and the results
/// relevant on large repos, matching what `rg`/`fd` users expect.
fn walker(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .require_git(false)
        .sort_by_file_path(|a, b| a.cmp(b)); // stable, alphabetical traversal
    builder
}

/// File-name search (fd-style): walk respecting `.gitignore`, match the name.
pub fn search_files(root: &Path, query: &str) -> Vec<PathBuf> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let smart = SmartCase::new(query);
    let mut out = Vec::new();
    for dent in walker(root).build().flatten() {
        if out.len() >= MAX_RESULTS {
            break;
        }
        if !dent.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let name = dent.file_name().to_string_lossy();
        if smart.is_match(&name) {
            if let Ok(rel) = dent.path().strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    out
}

/// Content search (ripgrep-style): the query is a smart-case regex; each matching
/// line becomes a [`ContentMatch`]. Invalid regex → no results (the UI shows 0).
pub fn search_content(root: &Path, query: &str) -> Vec<ContentMatch> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let matcher = match RegexMatcherBuilder::new().case_smart(true).build(query) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<ContentMatch> = Vec::new();
    for dent in walker(root).build().flatten() {
        if out.len() >= MAX_RESULTS {
            break;
        }
        if !dent.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = dent.path().strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_path_buf();
        let mut searcher = SearcherBuilder::new().build();
        let _ = searcher.search_path(
            &matcher,
            dent.path(),
            UTF8(|lnum, line| {
                out.push(ContentMatch {
                    path: rel.clone(),
                    line: lnum,
                    text: line.trim_end().to_string(),
                });
                Ok(out.len() < MAX_RESULTS)
            }),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {\n    greet();\n}\n").unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn greet() {\n    println!(\"Hello\");\n}\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "# Greeting demo\n").unwrap();
        // Ignored content must not appear in results.
        fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.join("ignored.txt"), "greet greet greet\n").unwrap();
        dir
    }

    #[test]
    fn file_search_is_smart_case_and_respects_gitignore() {
        let dir = fixture();
        let root = dir.path();

        let hits = search_files(root, "rs");
        assert!(hits.contains(&PathBuf::from("src/main.rs")));
        assert!(hits.contains(&PathBuf::from("src/lib.rs")));

        // Lowercase query is case-insensitive (matches README.md).
        assert!(search_files(root, "readme").contains(&PathBuf::from("README.md")));
        // Uppercase query is case-sensitive: "MAIN" matches nothing.
        assert!(search_files(root, "MAIN").is_empty());
        // Ignored files are skipped.
        assert!(search_files(root, "ignored").is_empty());
    }

    #[test]
    fn content_search_finds_matches_with_line_numbers() {
        let dir = fixture();
        let root = dir.path();

        let hits = search_content(root, "greet");
        // main.rs line 2 + lib.rs line 1, never the ignored file.
        assert!(hits
            .iter()
            .any(|m| m.path == Path::new("src/main.rs") && m.line == 2));
        assert!(hits
            .iter()
            .any(|m| m.path == Path::new("src/lib.rs") && m.line == 1));
        assert!(hits.iter().all(|m| m.path != Path::new("ignored.txt")));
    }

    #[test]
    fn content_search_smart_case() {
        let dir = fixture();
        let root = dir.path();
        // Uppercase in query → case-sensitive: "Hello" matches, "HELLO" doesn't.
        assert!(!search_content(root, "Hello").is_empty());
        assert!(search_content(root, "HELLO").is_empty());
    }

    #[test]
    fn empty_query_returns_nothing() {
        let dir = fixture();
        assert!(search_files(dir.path(), "  ").is_empty());
        assert!(search_content(dir.path(), "").is_empty());
    }
}
