# gdiff

A read-only terminal UI for **reviewing git diffs** — with the clarity of GitHub's diff view
and the rendering quality of [`delta`](https://github.com/dandavison/delta). Built for one
workflow: watching and reviewing the changes an AI coding agent (like Claude Code) makes —
live as it edits, or as the PR it opened.

> Status: v1 functionally complete (milestones M0–M7). A few DX extras are
> backlogged (config-file loading, copy, context-expand, mouse) — see `PLAN.md`.

## Features

- File tree on the left, diff on the right — GitHub-like.
- **Unified or side-by-side** view, toggleable.
- **delta-style rendering**: background-colored add/remove lines, word-level intra-line
  highlighting, and syntax highlighting.
- **Smart-but-explicit comparison**: by default shows your uncommitted changes vs `HEAD`;
  one key (`c`) to compare against a branch, a commit range, or a PR. Optional `--smart`
  auto-selection (uncommitted → branch-vs-base → PR).
- **Live auto-refresh** as files change on disk.
- **Review tracking**: mark files reviewed; persists per-repo; flags files that changed
  after you reviewed them.
- Theme auto-detected from your terminal background (light/dark).
- In-diff search (`/`), open-in-editor (`e`), help overlay (`?`).

Read-only by design — gdiff never modifies your repo.

## Usage

```bash
gdiff                  # uncommitted changes vs HEAD
gdiff main             # working tree vs main
gdiff main..feature    # a commit range
gdiff --staged         # staged changes
gdiff --pr             # the current branch's PR (via gh)
gdiff --smart          # auto-pick what to compare
```

## Install

```bash
cargo build --release   # binary at target/release/gdiff
```
Requires a Rust toolchain. `git` and `gh` (for PR mode) should be on PATH.

## Development

This repo is built primarily by AI coding agents. If that's you (or your agent), start with
**`AGENTS.md`**, then `DESIGN.md` and `PLAN.md`. Architecture: `docs/architecture.md`.
Testing (including how to verify the TUI headlessly): `docs/testing.md`.

```bash
cargo run                      # launch the TUI
cargo run --example render     # render a frame to stdout as text
cargo test                     # unit + snapshot tests
cargo fmt && cargo clippy --all-targets -- -D warnings
```
