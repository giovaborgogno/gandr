# AGENTS.md — gdiff

Guide for AI coding agents (Claude Code & others) working in this repo. Humans: see `README.md`.
`CLAUDE.md` is a symlink to this file.

## What this project is

`gdiff` is a **read-only** TUI (ratatui) for reviewing git diffs — delta-style rendering,
GitHub-like side-by-side, built for reviewing AI-coding-agent changes. Read `DESIGN.md`
(what we're building — **locked**) and `PLAN.md` (the milestone roadmap + current status)
before doing anything. Architecture lives in `docs/architecture.md`; the *why* behind key
choices is in `docs/decisions/`.

## Golden rules

1. **The UI is invisible to you — verify it headlessly.** Never claim a UI change works
   from reading code. Render it: snapshot tests (ratatui `TestBackend` + `insta`) or
   `cargo run --example render`. See `docs/testing.md`. This is the #1 rule.
2. **The UI never touches git2 directly.** All git access goes through the `GitBackend`
   trait (`src/git/mod.rs`). Diff/UI code depends on the trait + DTOs, never on `git2::*`.
3. **Read-only.** gdiff must never mutate the working tree, index, or refs. No staging,
   committing, or writing files in the target repo (review state under `.git/gdiff/` is the
   only exception).
4. **English everywhere** — code, comments, UI strings, docs, commit messages.
5. **No `unwrap`/`expect`/`panic!` in app/runtime code.** Use `anyhow::Result` and `?`.
   Panics are fine in tests only.
6. **Follow the milestones.** Pick the next unchecked item in `PLAN.md`; don't skip ahead
   or expand scope. Design changes need an ADR (`docs/decisions/`) + a `DESIGN.md` update.

## Setup

```bash
rustup show                 # toolchain (stable). git2 builds vendored libgit2 (needs cc/cmake).
cargo build                 # first build compiles libgit2 + syntect — slow once, then cached
```
External tools used at runtime: `git` (libgit2 is vendored, but base detection shells out
where simpler) and `gh` (PR mode). Both are expected on PATH.

## Build / test / lint commands

```bash
cargo build                 # debug build
cargo run                   # launch the TUI (needs a real terminal — see "Running")
cargo run --example render  # render a frame to stdout as text (agent-friendly!)
cargo test                  # unit + snapshot/integration tests
cargo insta test            # run snapshot tests (if cargo-insta installed)
cargo insta review          # review pending snapshots interactively
cargo fmt                   # format (also runs automatically via the PostToolUse hook)
cargo fmt --check           # CI check
cargo clippy --all-targets -- -D warnings   # lint; warnings are errors
```

## Running the TUI

`cargo run` needs an interactive terminal; you generally **cannot** drive it directly.
To verify behavior, do one of:
- **Snapshot test** (preferred): construct `App`, feed synthetic key events, render to
  `TestBackend`, assert with `insta`. Deterministic, no terminal needed.
- **`cargo run --example render`**: prints a frame to stdout so you can read the UI as
  text. (M0 renders an empty `App`; fixture-backed scenarios are added as the UI grows.)
- The `/verify-tui` skill wraps these. Do not block on a live `cargo run`.

## Testing strategy (see docs/testing.md for detail)

- **Unit**: diff engine, base detection, config, tree-building — pure functions, table tests.
- **Fixtures**: `testutil` builds temp git repos with known changes (`tempfile` + `git2`).
  Never test against the gdiff repo's own working tree (non-deterministic).
- **Snapshot/e2e**: drive `App` with key-event sequences → render → `insta` golden.
  This is our deterministic "e2e" — no pty required.
- A change to rendering output should update snapshots **intentionally** (`cargo insta
  review`), never blindly `--accept` without reading the diff.

## Definition of done (run before every commit)

```bash
cargo fmt --check && \
cargo clippy --all-targets -- -D warnings && \
cargo test
```
All green. Snapshots updated on purpose. The relevant `PLAN.md` checkboxes ticked.

**Then run `/code-review` and address its findings before committing.** This is
mandatory for every commit — review the pending diff, fix real issues (or consciously
decline with a reason), and only then commit. Don't skip it because "it's just docs" or
"a small change". (`/code-review` is a *local* pre-commit gate run by the agent; CI only
runs fmt/clippy/test and cannot run it.)

## Workflow (git)

- **Commit directly to `main`, one commit per milestone** (or per coherent sub-task).
  No PRs for gdiff's own development (see ADR 0005).
- **Run `/code-review` before every commit** and act on its findings (see "Definition of done").
- Conventional commits: `feat(ui): …`, `fix(diff): …`, `test:`, `docs:`, `chore:`, `refactor:`.
- Update `PLAN.md` (tick boxes) in the same commit as the work it tracks.
- Commit messages end with the Co-Authored-By trailer per the harness instructions.

## Code style / conventions

- Rust 2021, `rustfmt` defaults, `clippy -D warnings`.
- Module boundaries are the architecture in `PLAN.md`/`docs/architecture.md`. Keep `git/`,
  `diff/`, `highlight/`, `ui/`, `app/` decoupled in that dependency direction
  (`ui` → `diff`/`highlight` → `git` trait; nothing depends back upward).
- Errors: `anyhow` at boundaries; prefer typed errors only where callers branch on them.
- Keep functions render-pure where possible (input state → output `Vec<Line>`), so they're
  snapshot-testable without a terminal.
- Match surrounding style; small, reviewable commits.

## Things that bite (gotchas)

- **imara-diff 0.2** changed its API vs 0.1 — check the installed source/docs, don't trust
  memory. Use it for both line-level and word-level (different token granularity).
- **ratatui 0.30** + **crossterm 0.29** — verify widget/API signatures against installed
  docs; the ecosystem moves fast.
- **syntect**: we use `default-features = false` + `default-fancy` (no oniguruma C dep) and
  `two-face` for extra syntaxes/themes. Theme loading is not free — cache it.
- **termbg / OSC 11** must run *before* entering raw mode / alt-screen, once at startup.
- First `cargo build` is slow (vendored libgit2 + syntect). That's expected, not a hang.

## Skills available

- `/next-milestone` — pick the next `PLAN.md` milestone, implement, gate, tick, commit.
- `/verify-tui` — render a scenario/state to text to inspect the UI headlessly.
- `/new-fixture` — scaffold a test fixture repo scenario.

Nested `AGENTS.md` files may appear under `src/ui/` and `src/git/` with local rules; the
nearest one wins.
