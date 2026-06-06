---
name: new-fixture
description: Scaffold a deterministic temp git repo fixture for testing gdiff (a known set of changes — modify, add, delete, rename, binary, multi-hunk). Use when writing tests that need a repo with specific diffs.
---

# new-fixture

Fixtures are small, deterministic git repos built at test time with `git2` + `tempfile`, so
tests never depend on gdiff's own (changing) working tree.

## Use the `testutil` helper

```rust
use crate::testutil::Fixture;

let fx = Fixture::new();                       // fresh temp repo, one initial commit
fx.write("src/a.rs", "fn main() {}\n");
fx.commit("init");

// Now create the change you want to diff:
fx.write("src/a.rs", "fn main() { let x = 1; }\n");   // modify (uncommitted)
fx.add("src/b.rs", "// new file\n");                  // add
fx.remove("old.rs");                                  // delete
fx.rename("a.rs", "renamed.rs");                      // rename

let backend = Git2Backend::open(fx.path())?;
let files = backend.changed_files(&CompareSpec::Uncommitted)?;
```

## Guidelines
- Keep contents **small and obvious** — fixtures feed snapshots, which humans/agents read.
- Cover the cases that matter per milestone: modify, add, delete, rename, binary, and a
  multi-hunk file. (See `docs/testing.md` → "What good coverage looks like".)
- One fixture per scenario; don't overload a single fixture with everything.
- If `testutil` lacks a helper you need (e.g. staged vs unstaged, a commit range), add it to
  `testutil` rather than hand-rolling git2 calls inline in the test.
