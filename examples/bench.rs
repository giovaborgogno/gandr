//! Micro-benchmarks for gandr's hot paths. Run with `cargo run --release --example bench`.

use gandr::diff::fold;
use gandr::diff::{engine, FileDiff};
use gandr::git::git2_backend::Git2Backend;
use gandr::git::CompareSpec;
use gandr::highlight::{Highlighter, Palette, ThemeMode};
use gandr::testutil::Fixture;
use gandr::{browser::Browser, ui::viewer_unified};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

fn bench<T>(label: &str, f: impl Fn() -> T) -> T {
    // Warm once, then take the best of 5 runs.
    let mut out = f();
    let mut best = std::time::Duration::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        out = f();
        best = best.min(t.elapsed());
    }
    println!("{label:<48} {best:>10.2?}");
    out
}

fn main() {
    // 1) One large file with many changes (a big single-file diff).
    let fx = Fixture::new();
    let base: String = (0..5000).map(|i| format!("line {i}\n")).collect();
    fx.write("big.txt", &base);
    fx.commit("init");
    let edited: String = (0..5000)
        .map(|i| {
            if i % 10 == 0 {
                format!("LINE {i} changed\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect();
    fx.write("big.txt", &edited);
    let backend = Git2Backend::open(fx.path()).unwrap();
    let d = bench("build_diffs: 5000-line file, ~500 changed lines", || {
        engine::build_diffs(&backend, &CompareSpec::Uncommitted, 3).unwrap()
    });
    let hunks: usize = d.iter().map(|f| f.hunks.len()).sum();
    println!("    └ {} file(s), {hunks} hunks", d.len());

    // 2) Many changed files.
    let fx2 = Fixture::new();
    for i in 0..500 {
        fx2.write(&format!("src/f{i:03}.rs"), "pub fn a() {}\n");
    }
    fx2.commit("init");
    for i in 0..500 {
        fx2.write(
            &format!("src/f{i:03}.rs"),
            "pub fn a() {\n    let x = 1;\n}\n",
        );
    }
    let backend2 = Git2Backend::open(fx2.path()).unwrap();
    let d2 = bench("build_diffs: 500 changed files", || {
        engine::build_diffs(&backend2, &CompareSpec::Uncommitted, 3).unwrap()
    });
    println!("    └ {} files", d2.len());

    // 3) File-browser tree build over the repo root.
    let cwd = std::env::current_dir().unwrap();
    let browser = Browser::new(cwd.clone());
    let rows = bench("browser rows() at repo root (cold cache)", || {
        let b = Browser::new(cwd.clone());
        b.rows()
    });
    println!("    └ {} visible rows", rows.len());
    let _ = browser;

    // 4) The work done when SELECTING a large file in the diff viewer (all on the
    //    UI thread, cached after the first hit): full annotated lines, both-side
    //    stateful highlight, and folding. This is the "no lag on navigation" path.
    let big: &FileDiff = &d[0];
    let full = bench("all_lines: 5000-line file (per-selection)", || {
        engine::all_lines(&big.old_text, &big.new_text)
    });
    println!("    └ {} lines", full.len());

    let hl = Highlighter::for_path(Path::new("big.rs"), ThemeMode::Dark);
    bench(
        "highlight_file ×2 sides: 5000 lines (per-selection)",
        || {
            let o = hl.highlight_file(&engine::split_lines(&big.old_text));
            let n = hl.highlight_file(&engine::split_lines(&big.new_text));
            (o, n)
        },
    );

    let display = bench("fold: 5000-line file (per render/key)", || {
        fold::fold(&full, 3, &HashMap::new())
    });
    println!("    └ {} display rows", display.len());

    // 5) Building the unified viewer rows for the visible window (per frame).
    let palette = Palette::for_mode(ThemeMode::Dark);
    let o_hl = hl.highlight_file(&engine::split_lines(&big.old_text));
    let n_hl = hl.highlight_file(&engine::split_lines(&big.new_text));
    bench("viewer rows(): unified, full file (per frame)", || {
        viewer_unified::rows(&full, &display, 120, &o_hl, &n_hl, &palette, true, None)
    });

    // 6) Diff-tree row building for a big changeset (rebuilt while navigating).
    let many: Vec<gandr::diff::FileDiff> = (0..1000)
        .map(|i| make_file(&format!("crate{}/src/mod{i}/file{i}.rs", i % 20)))
        .collect();
    let collapsed = std::collections::HashSet::new();
    let trows = bench("tree::build_rows: 1000-file changeset", || {
        gandr::ui::tree::build_rows(&many, &collapsed)
    });
    println!("    └ {} tree rows", trows.len());

    // 7) Real-repo navigation costs (set GANDR_BENCH_REPO=/path/to/big/checkout).
    if let Ok(repo) = std::env::var("GANDR_BENCH_REPO") {
        let root = std::path::PathBuf::from(&repo);
        println!("-- real repo: {repo} --");
        // The per-cursor-move cost in the Repo browser is read + highlight.
        let mut sizes: Vec<(std::path::PathBuf, usize)> = Vec::new();
        for p in ["src/main.ts", "src/vscode-dts/vscode.d.ts"] {
            let f = root.join(p);
            if let Ok(t) = std::fs::read_to_string(&f) {
                sizes.push((f, t.lines().count()));
            }
        }
        for (f, n) in &sizes {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            bench(&format!("  fs::read {name} ({n} lines)"), || {
                std::fs::read_to_string(f).unwrap()
            });
            let text = std::fs::read_to_string(f).unwrap();
            let lines = engine::split_lines(&text);
            let h = Highlighter::for_path(f, ThemeMode::Dark);
            bench(&format!("  highlight_file {name} ({n} lines)"), || {
                h.highlight_file(&lines)
            });
        }
    } else {
        println!("(set GANDR_BENCH_REPO to measure on a real checkout)");
    }
}

/// A trivial one-hunk FileDiff at `path`, for tree-building benchmarks.
fn make_file(path: &str) -> gandr::diff::FileDiff {
    use gandr::diff::engine::build_file_diff;
    use gandr::git::{FileChange, Status};
    let change = FileChange {
        path: std::path::PathBuf::from(path),
        old_path: None,
        status: Status::Modified,
        is_binary: false,
        additions: 0,
        deletions: 0,
    };
    build_file_diff(change, Some(b"a\n"), Some(b"b\n"), 3)
}
