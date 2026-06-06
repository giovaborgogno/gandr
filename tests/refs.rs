//! Tests for backend ref listing (M9). No TUI — validates the data the fuzzy
//! ref picker consumes.

use gandr::git::git2_backend::Git2Backend;
use gandr::git::{GitBackend, RefKind};
use gandr::testutil::Fixture;

#[test]
fn list_refs_groups_branches_then_tags_each_sorted() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init"); // on `main`
    fx.checkout_new_branch("feature/login");
    fx.checkout_new_branch("release-1.0");
    fx.tag("v1.0.0");
    fx.tag("v0.9.0");

    let backend = Git2Backend::open(fx.path()).unwrap();
    let refs = backend.list_refs().unwrap();

    // Local branches come first (alphabetical), then tags (alphabetical).
    let branches: Vec<_> = refs
        .iter()
        .filter(|r| r.kind == RefKind::LocalBranch)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(branches, ["feature/login", "main", "release-1.0"]);

    let tags: Vec<_> = refs
        .iter()
        .filter(|r| r.kind == RefKind::Tag)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(tags, ["v0.9.0", "v1.0.0"]);

    // Branches are ordered before tags overall.
    let first_tag = refs.iter().position(|r| r.kind == RefKind::Tag).unwrap();
    let last_branch = refs
        .iter()
        .rposition(|r| r.kind == RefKind::LocalBranch)
        .unwrap();
    assert!(last_branch < first_tag);
}
