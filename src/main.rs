//! gdiff binary entry point. Thin wrapper around the library; all logic lives in
//! the `gdiff` crate so it's testable headlessly (see docs/testing.md).

use anyhow::Result;
use gdiff::config::Config;

fn main() -> Result<()> {
    // Full argument grammar (refs, ranges, --pr, --smart) lands in M5; for now the
    // CLI yields the default Uncommitted comparison.
    let invocation = gdiff::cli::parse();
    gdiff::app::run(Config::default(), invocation.spec)
}
