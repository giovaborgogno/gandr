//! Unit tests for CLI argument parsing.

use gandr::cli::parse_args;
use gandr::git::CompareSpec;

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[test]
fn default_is_uncommitted() {
    let inv = parse_args(vec![]);
    assert_eq!(inv.spec, CompareSpec::Uncommitted);
    assert!(!inv.smart);
}

#[test]
fn staged_and_smart_flags() {
    let inv = parse_args(args(&["--staged", "--smart"]));
    assert_eq!(inv.spec, CompareSpec::Staged);
    assert!(inv.smart);
}

#[test]
fn a_ref_becomes_workdir_vs() {
    let inv = parse_args(args(&["main"]));
    assert_eq!(inv.spec, CompareSpec::WorkdirVs("main".into()));
}

#[test]
fn range_syntax() {
    let inv = parse_args(args(&["main..feature"]));
    assert_eq!(
        inv.spec,
        CompareSpec::Range("main".into(), "feature".into())
    );
}

#[test]
fn pr_with_and_without_number() {
    assert_eq!(parse_args(args(&["--pr"])).spec, CompareSpec::Pr(None));
    assert_eq!(
        parse_args(args(&["--pr", "123"])).spec,
        CompareSpec::Pr(Some(123))
    );
}
