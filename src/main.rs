//! gdiff binary entry point. Thin wrapper around the library; all logic lives in
//! the `gdiff` crate so it's testable headlessly (see docs/testing.md).

use anyhow::Result;
use gdiff::config::Config;

fn main() -> Result<()> {
    // M0: ignore CLI and run with defaults. Argument parsing → CompareSpec lands in M5.
    let _invocation = gdiff::cli::parse();
    gdiff::app::run(Config::default())
}
