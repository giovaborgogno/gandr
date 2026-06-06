//! Command-line parsing → a [`CompareSpec`] plus flags.
//!
//! Grammar:
//! ```text
//! gandr                 # Uncommitted (default)
//! gandr <ref>           # working tree vs <ref>
//! gandr <ref>..<ref>    # commit range
//! gandr --staged        # index vs HEAD
//! gandr --pr [N]        # PR via gh (current branch if N omitted)
//! gandr --smart         # enable smart auto-selection
//! ```
//! Path scoping (`gandr <path>`) is deferred (see PLAN backlog): a bare positional
//! is treated as a ref, since ref/path disambiguation needs a `--` convention.

use crate::git::CompareSpec;

/// A parsed invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub spec: CompareSpec,
    /// Enable smart auto-selection for this run.
    pub smart: bool,
}

impl Default for Invocation {
    fn default() -> Self {
        Self {
            spec: CompareSpec::Uncommitted,
            smart: false,
        }
    }
}

/// Parse the process arguments (excluding argv[0]).
pub fn parse() -> Invocation {
    parse_args(std::env::args().skip(1).collect())
}

/// Parse from an explicit argument list (testable).
pub fn parse_args(args: Vec<String>) -> Invocation {
    let mut inv = Invocation::default();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--staged" => inv.spec = CompareSpec::Staged,
            "--smart" => inv.smart = true,
            "--pr" => {
                inv.spec = CompareSpec::Pr(None);
                if let Some(next) = args.get(i + 1) {
                    if let Ok(n) = next.parse::<u32>() {
                        inv.spec = CompareSpec::Pr(Some(n));
                        i += 1;
                    }
                }
            }
            a if a.starts_with('-') => {} // ignore unknown flags
            a => {
                inv.spec = match a.split_once("..") {
                    Some((l, r)) => CompareSpec::Range(l.to_string(), r.to_string()),
                    None => CompareSpec::WorkdirVs(a.to_string()),
                };
            }
        }
        i += 1;
    }
    inv
}
