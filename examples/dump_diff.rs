//! Print computed diffs as text, to eyeball the engine output without a TUI.
//!
//! `cargo run --example dump_diff` builds a small fixture repo and prints its
//! uncommitted diff in a git-like form. A debugging aid for the diff engine (M1).

use anyhow::Result;
use gdiff::diff::{engine, LineKind};
use gdiff::git::git2_backend::Git2Backend;
use gdiff::git::CompareSpec;
use gdiff::testutil::Fixture;

fn main() -> Result<()> {
    let fx = Fixture::new();
    fx.write("src/main.rs", "fn main() {\n    println!(\"hi\");\n}\n");
    fx.write("README.md", "# demo\n\nold line\n");
    fx.commit("init");

    // Introduce a mix of changes.
    fx.write(
        "src/main.rs",
        "fn main() {\n    let name = \"world\";\n    println!(\"hi {name}\");\n}\n",
    );
    fx.write("README.md", "# demo\n\nnew line\n");
    fx.write("notes.txt", "a brand new file\n");

    let backend = Git2Backend::open(fx.path())?;
    let diffs = engine::build_diffs(&backend, &CompareSpec::Uncommitted, 3)?;

    for d in &diffs {
        println!(
            "── {} [{}]  +{} -{}",
            d.change.path.display(),
            d.change.status.marker(),
            d.change.additions,
            d.change.deletions
        );
        for hunk in &d.hunks {
            println!("{}", hunk.header);
            for line in &hunk.lines {
                let sign = match line.kind {
                    LineKind::Add => '+',
                    LineKind::Del => '-',
                    LineKind::Context => ' ',
                };
                println!("{sign} {}", line.text);
            }
        }
        println!();
    }
    Ok(())
}
