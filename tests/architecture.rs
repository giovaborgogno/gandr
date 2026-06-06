//! Architecture guardrails enforced as tests, so future agents can't quietly
//! violate the layering rules in AGENTS.md.
//!
//! Golden rule #2: the UI/diff/highlight/app layers never touch `git2` directly —
//! all git access goes through the `GitBackend` trait. Only `src/git/` may name `git2`.

use std::fs;
use std::path::Path;

/// Recursively collect `.rs` files under `dir`, skipping `src/git/` (the only place
/// allowed to depend on git2).
fn rust_files_outside_git(dir: &Path, git_dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path == git_dir {
                continue;
            }
            rust_files_outside_git(&path, git_dir, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// A line that uses git2 in code (not in a comment). We strip line/doc comments
/// (`//`, `///`, `//!`) to avoid flagging prose that merely mentions the rule.
fn uses_git2_in_code(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return false;
    }
    // Drop any trailing `// ...` comment before checking.
    let code = trimmed.split("//").next().unwrap_or("");
    code.contains("git2")
}

#[test]
fn only_the_git_module_depends_on_git2() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let git_dir = src.join("git");

    let mut files = Vec::new();
    rust_files_outside_git(&src, &git_dir, &mut files);

    let mut violations = Vec::new();
    for file in &files {
        let content = fs::read_to_string(file).unwrap_or_default();
        for (i, line) in content.lines().enumerate() {
            if uses_git2_in_code(line) {
                violations.push(format!("{}:{} → {}", file.display(), i + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "git2 must only be used inside src/git/ (GitBackend trait seam). Violations:\n{}",
        violations.join("\n")
    );
}
