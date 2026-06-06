//! Command-line parsing → a [`CompareSpec`] plus flags.
//!
//! M0 ships a trivial default (the bare `gdiff` behaviour); the full grammar
//! (`<ref>`, `<ref>..<ref>`, `--staged`, `--pr`, `--smart`, `<path>`) lands in M5.

use crate::git::CompareSpec;
use std::path::PathBuf;

/// A parsed invocation.
#[derive(Debug, Clone)]
pub struct Invocation {
    pub spec: CompareSpec,
    /// Enable smart auto-selection for this run.
    pub smart: bool,
    /// Scope the comparison to a path.
    pub path: Option<PathBuf>,
}

impl Default for Invocation {
    fn default() -> Self {
        Self {
            spec: CompareSpec::Uncommitted,
            smart: false,
            path: None,
        }
    }
}

/// Parse process arguments. M0: always the default invocation.
pub fn parse() -> Invocation {
    Invocation::default()
}
