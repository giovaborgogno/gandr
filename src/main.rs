//! gandr binary entry point. Thin wrapper around the library; all logic lives in
//! the `gandr` crate so it's testable headlessly (see docs/testing.md).

use anyhow::Result;
use gandr::config::Config;

fn main() -> Result<()> {
    let invocation = gandr::cli::parse();
    gandr::app::run(Config::default(), invocation)
}
