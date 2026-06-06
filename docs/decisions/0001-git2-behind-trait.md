# 0001 — git2 backend behind a `GitBackend` trait

## Context
We need structured diff data (changed files, hunks, file contents) without parsing textual
`git` output. Candidates: `git2` (libgit2 bindings, mature, ergonomic, used by gitui) and
`gix` (gitoxide, pure-Rust, fast on huge repos, lower-level for working-tree status).

## Decision
Ship a single **`git2`** implementation in v1, but put **all** git access behind a
`GitBackend` trait so no other module depends on `git2::*`.

## Consequences
- Fast start: `git2`'s diff/status APIs are ergonomic for our comparisons.
- The word/line diff is computed by us (imara-diff, ADR 0002), not by the backend, so the
  backend's job is small: list changed files + fetch old/new contents + resolve refs.
- Migrating to `gix` later (for very large repos) touches only `src/git/`, not diff/UI.
- libgit2 is vendored by the `git2` crate → first build is slow but no system lib needed.
