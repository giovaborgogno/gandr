#![cfg(unix)]

use gandr::git::base;
use gandr::git::git2_backend::Git2Backend;
use gandr::git::{CompareSpec, GitBackend};
use gandr::testutil::Fixture;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn pr_diff_excludes_changes_only_on_the_base_branch() {
    let fx = Fixture::new();
    fx.write("skill.md", "original guidance\n");
    fx.write("feature.txt", "original feature\n");
    let ancestor = fx.commit("initial");
    fx.checkout_new_branch("feature");
    fx.write("feature.txt", "PR feature\n");
    let head = fx.commit("feature change");

    let checkout = Command::new("git")
        .args(["checkout", "main"])
        .current_dir(fx.path())
        .output()
        .unwrap();
    assert!(checkout.status.success());
    fx.write("skill.md", "updated guidance\n");
    fx.write("feature.txt", "base feature\n");
    let base = fx.commit("unrelated base changes");

    let bin = tempfile::tempdir().unwrap();
    let gh = bin.path().join("gh");
    std::fs::write(
        &gh,
        format!("#!/bin/sh\nprintf '123\t{base}\t{head}\tFeature change\\n'\n"),
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old_path = std::env::var_os("PATH");
    std::env::set_var("PATH", bin.path());

    let backend = Git2Backend::open(fx.path()).unwrap();
    for number in [Some(123), None] {
        let resolved = base::resolve(&backend, CompareSpec::Pr(number), false, &[]).unwrap();
        assert_eq!(
            resolved.spec,
            CompareSpec::Range(ancestor.to_string(), head.to_string())
        );
        assert_eq!(resolved.title.as_deref(), Some("PR #123: Feature change"));
        let files = backend.changed_files(&resolved.spec).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.to_str(), Some("feature.txt"));
        let (old, new) = backend.file_contents(&resolved.spec, &files[0]).unwrap();
        assert_eq!(old.unwrap(), b"original feature\n");
        assert_eq!(new.unwrap(), b"PR feature\n");
    }

    let direct = backend
        .changed_files(&CompareSpec::Range(base.to_string(), head.to_string()))
        .unwrap();
    assert_eq!(direct.len(), 2);
    assert_eq!(
        backend
            .merge_base(&ancestor.to_string(), &head.to_string())
            .unwrap(),
        ancestor.to_string()
    );
    assert!(backend
        .merge_base("missing-revision", &head.to_string())
        .is_err());

    match old_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
}
