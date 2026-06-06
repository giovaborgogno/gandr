//! gdiff binary entry point. Thin wrapper around the library; all logic lives in
//! the `gdiff` crate so it's testable headlessly (see docs/testing.md).

use anyhow::Result;
use gdiff::config::Config;

fn main() -> Result<()> {
    let invocation = gdiff::cli::parse();
    gdiff::app::run(Config::default(), invocation)
}
