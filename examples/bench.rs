//! Micro-benchmarks for gdiff's hot paths. Run with `cargo run --release --example bench`.

use gdiff::diff::fold;
use gdiff::diff::{engine, FileDiff};
use gdiff::git::git2_backend::Git2Backend;
use gdiff::git::CompareSpec;
use gdiff::highlight::{Highlighter, Palette, ThemeMode};
use gdiff::testutil::Fixture;
use gdiff::{browser::Browser, ui::viewer_unified};
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
}
