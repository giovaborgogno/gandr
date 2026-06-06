//! Micro-benchmarks for gdiff's hot paths. Run with `cargo run --release --example bench`.

use gdiff::browser::Browser;
use gdiff::diff::engine;
use gdiff::git::git2_backend::Git2Backend;
use gdiff::git::CompareSpec;
use gdiff::testutil::Fixture;
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
}
