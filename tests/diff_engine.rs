//! Unit/integration tests for the diff engine against deterministic fixtures.
//! No TUI — this validates that we produce correct hunks before rendering them.

use gdiff::diff::engine;
use gdiff::diff::LineKind;
use gdiff::git::git2_backend::Git2Backend;
use gdiff::git::{CompareSpec, GitBackend, Status};
use gdiff::testutil::Fixture;
use std::path::Path;

const CTX: usize = 3;

/// Build the uncommitted diffs for a fixture.
fn diffs(fx: &Fixture) -> Vec<gdiff::diff::FileDiff> {
    diffs_spec(fx, &CompareSpec::Uncommitted)
}

/// Build the diffs for a fixture under an arbitrary comparison.
fn diffs_spec(fx: &Fixture, spec: &CompareSpec) -> Vec<gdiff::diff::FileDiff> {
    let backend = Git2Backend::open(fx.path()).unwrap();
    engine::build_diffs(&backend, spec, CTX).unwrap()
}

fn texts(lines: &[gdiff::diff::Line], kind: LineKind) -> Vec<String> {
    lines
        .iter()
        .filter(|l| l.kind == kind)
        .map(|l| l.text.clone())
        .collect()
}

#[test]
fn modify_produces_del_then_add_with_context() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo\nthree\n");
    fx.commit("init");
    fx.write("a.txt", "one\nTWO\nthree\n");

    let diffs = diffs(&fx);
    assert_eq!(diffs.len(), 1);
    let d = &diffs[0];
    assert_eq!(d.change.path, Path::new("a.txt"));
    assert_eq!(d.change.status, Status::Modified);
    assert_eq!(d.change.additions, 1);
    assert_eq!(d.change.deletions, 1);

    assert_eq!(d.hunks.len(), 1);
    let lines = &d.hunks[0].lines;
    assert_eq!(texts(lines, LineKind::Del), vec!["two"]);
    assert_eq!(texts(lines, LineKind::Add), vec!["TWO"]);
    // Surrounding context preserved.
    assert_eq!(texts(lines, LineKind::Context), vec!["one", "three"]);

    // Line numbers: del on old line 2, add on new line 2.
    let del = lines.iter().find(|l| l.kind == LineKind::Del).unwrap();
    assert_eq!(del.old_no, Some(2));
    assert_eq!(del.new_no, None);
    let add = lines.iter().find(|l| l.kind == LineKind::Add).unwrap();
    assert_eq!(add.new_no, Some(2));
    assert_eq!(add.old_no, None);
}

#[test]
fn added_file_is_all_additions() {
    let fx = Fixture::new();
    fx.write("base.txt", "x\n");
    fx.commit("init");
    fx.write("new.txt", "alpha\nbeta\n");

    let diffs = diffs(&fx);
    let d = diffs
        .iter()
        .find(|d| d.change.path == Path::new("new.txt"))
        .unwrap();
    assert_eq!(d.change.status, Status::Added);
    assert_eq!(d.change.additions, 2);
    assert_eq!(d.change.deletions, 0);
    assert_eq!(
        texts(&d.hunks[0].lines, LineKind::Add),
        vec!["alpha", "beta"]
    );
}

#[test]
fn deleted_file_is_all_deletions() {
    let fx = Fixture::new();
    fx.write("gone.txt", "a\nb\nc\n");
    fx.commit("init");
    fx.remove("gone.txt");

    let diffs = diffs(&fx);
    let d = diffs
        .iter()
        .find(|d| d.change.path == Path::new("gone.txt"))
        .unwrap();
    assert_eq!(d.change.status, Status::Deleted);
    assert_eq!(d.change.deletions, 3);
    assert_eq!(d.change.additions, 0);
    assert_eq!(texts(&d.hunks[0].lines, LineKind::Del), vec!["a", "b", "c"]);
}

#[test]
fn rename_is_detected_with_old_path() {
    let fx = Fixture::new();
    let body = "line1\nline2\nline3\nline4\nline5\nline6\n";
    fx.write("orig.txt", body);
    fx.commit("init");
    fx.remove("orig.txt");
    fx.write("renamed.txt", body);
    fx.stage_all(); // make rename detection reliable across index + worktree

    let diffs = diffs(&fx);
    let d = diffs
        .iter()
        .find(|d| d.change.path == Path::new("renamed.txt"))
        .unwrap();
    assert_eq!(d.change.status, Status::Renamed);
    assert_eq!(d.change.old_path.as_deref(), Some(Path::new("orig.txt")));
}

#[test]
fn binary_file_has_no_hunks() {
    let fx = Fixture::new();
    fx.write("base.txt", "x\n");
    fx.commit("init");
    // Invalid UTF-8 with a NUL byte → binary.
    fx.write_bytes("blob.bin", &[0x00, 0xff, 0xfe, 0x10, 0x80]);

    let diffs = diffs(&fx);
    let d = diffs
        .iter()
        .find(|d| d.change.path == Path::new("blob.bin"))
        .unwrap();
    assert!(d.change.is_binary);
    assert!(d.hunks.is_empty());
}

#[test]
fn valid_utf8_with_nul_is_binary() {
    let fx = Fixture::new();
    fx.write("base.txt", "x\n");
    fx.commit("init");
    // Bytes are valid UTF-8 (NUL is a valid code point) but contain a NUL → binary.
    fx.write_bytes("withnul.dat", b"hello\x00world\n");

    let diffs = diffs(&fx);
    let d = diffs
        .iter()
        .find(|d| d.change.path == Path::new("withnul.dat"))
        .unwrap();
    assert!(d.change.is_binary);
    assert!(d.hunks.is_empty());
}

#[test]
fn far_apart_changes_make_two_hunks() {
    let fx = Fixture::new();
    let original: String = (1..=20).map(|n| format!("line{n}\n")).collect();
    fx.write("big.txt", &original);
    fx.commit("init");
    // Change line 2 and line 18 — far apart (gap > 2*ctx) → separate hunks.
    let edited = original
        .replace("line2\n", "LINE2\n")
        .replace("line18\n", "LINE18\n");
    fx.write("big.txt", &edited);

    let diffs = diffs(&fx);
    let d = &diffs[0];
    assert_eq!(d.hunks.len(), 2);
}

#[test]
fn nearby_changes_merge_into_one_hunk() {
    let fx = Fixture::new();
    let original: String = (1..=20).map(|n| format!("line{n}\n")).collect();
    fx.write("big.txt", &original);
    fx.commit("init");
    // Change line 8 and line 10 — gap (1 line) <= 2*ctx → one merged hunk.
    let edited = original
        .replace("line8\n", "LINE8\n")
        .replace("line10\n", "LINE10\n");
    fx.write("big.txt", &edited);

    let diffs = diffs(&fx);
    let d = &diffs[0];
    assert_eq!(d.hunks.len(), 1);
    // The unchanged line9 between them is shown as context.
    assert!(texts(&d.hunks[0].lines, LineKind::Context).contains(&"line9".to_string()));
}

// ---- M5: comparison kinds ----

#[test]
fn staged_shows_only_staged_changes() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "STAGED\n");
    fx.stage_all();
    fx.write("a.txt", "STAGED then unstaged\n"); // further unstaged edit

    let d = diffs_spec(&fx, &CompareSpec::Staged);
    let f = d
        .iter()
        .find(|f| f.change.path == Path::new("a.txt"))
        .unwrap();
    // Staged view compares index (STAGED) vs HEAD, ignoring the later unstaged edit.
    assert_eq!(texts(&f.hunks[0].lines, LineKind::Add), vec!["STAGED"]);
}

#[test]
fn commit_range_shows_changes_between_two_commits() {
    let fx = Fixture::new();
    fx.write("a.txt", "v1\n");
    let c1 = fx.commit("c1");
    fx.write("a.txt", "v2\n");
    let c2 = fx.commit("c2");

    let d = diffs_spec(&fx, &CompareSpec::Range(c1.to_string(), c2.to_string()));
    let f = &d[0];
    assert_eq!(texts(&f.hunks[0].lines, LineKind::Del), vec!["v1"]);
    assert_eq!(texts(&f.hunks[0].lines, LineKind::Add), vec!["v2"]);
}

#[test]
fn single_commit_shows_its_own_changes() {
    let fx = Fixture::new();
    fx.write("a.txt", "v1\n");
    fx.commit("c1");
    fx.write("a.txt", "v2\n");
    let c2 = fx.commit("c2");

    let d = diffs_spec(&fx, &CompareSpec::Commit(c2.to_string()));
    assert_eq!(texts(&d[0].hunks[0].lines, LineKind::Add), vec!["v2"]);
}

#[test]
fn workdir_vs_ref_compares_against_a_branch() {
    let fx = Fixture::new();
    fx.write("a.txt", "base\n");
    fx.commit("base"); // on main
    fx.checkout_new_branch("feature");
    fx.write("a.txt", "feature\n");
    fx.commit("feature change");

    let d = diffs_spec(&fx, &CompareSpec::WorkdirVs("main".into()));
    let f = &d[0];
    assert_eq!(texts(&f.hunks[0].lines, LineKind::Del), vec!["base"]);
    assert_eq!(texts(&f.hunks[0].lines, LineKind::Add), vec!["feature"]);
}

#[test]
fn detect_base_finds_merge_base_with_main() {
    let fx = Fixture::new();
    fx.write("a.txt", "base\n");
    let base_commit = fx.commit("base"); // on main
    fx.checkout_new_branch("feature");
    fx.write("a.txt", "feat\n");
    fx.commit("feat");

    let backend = Git2Backend::open(fx.path()).unwrap();
    let base = backend.detect_base(&["main".to_string()]).unwrap();
    assert_eq!(base.as_deref(), Some(base_commit.to_string().as_str()));
}

#[test]
fn context_is_capped_at_configured_lines() {
    let fx = Fixture::new();
    let original: String = (1..=20).map(|n| format!("line{n}\n")).collect();
    fx.write("big.txt", &original);
    fx.commit("init");
    let edited = original.replace("line10\n", "LINE10\n");
    fx.write("big.txt", &edited);

    let diffs = diffs(&fx);
    let ctx = texts(&diffs[0].hunks[0].lines, LineKind::Context);
    // 3 lines of context on each side of a single changed line.
    assert_eq!(
        ctx,
        vec!["line7", "line8", "line9", "line11", "line12", "line13"]
    );
}
