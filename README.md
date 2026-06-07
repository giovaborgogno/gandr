<div align="center">

# gandr

### Review code changes and browse your repo — at terminal speed.

A fast, read-only TUI for reviewing diffs and exploring repositories — the clarity of
GitHub's side-by-side view with the rendering of [`delta`](https://github.com/dandavison/delta).
Built for the AI-coding era: watch an agent's changes land **live**, review them, then search
the whole codebase — without leaving your terminal.

[![crates.io](https://img.shields.io/crates/v/gandr.svg?logo=rust)](https://crates.io/crates/gandr)
[![downloads](https://img.shields.io/crates/d/gandr.svg)](https://crates.io/crates/gandr)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg?logo=rust)

![gandr in action](https://github.com/user-attachments/assets/766c2fca-513c-4b36-9702-00eb565a49c9)

</div>

**Live review — your agent edits, you review.** As Claude Code (left) writes code, the changes
stream into gandr (right): review the diff and search the whole repo without breaking flow.

![gandr reviewing an agent's changes live](https://github.com/user-attachments/assets/c33a56db-5d6f-49bb-a750-cfac1cf37115)

## Why gandr?

- **Fast at scale.** Windowed rendering and cached trees keep navigation instant on huge
  repos (vscode's ~15.6k files, 1.3k-file diffs) — no lag per keystroke.
- **Two tools in one.** A **Diff** reviewer *and* a **Repo** browser, in one keyboard-driven TUI.
- **Built for agents.** Live auto-refresh + review tracking turn "my agent changed 30 files"
  into a calm, reviewable flow.
- **Zero external tools.** Diffing (libgit2), syntax highlighting (syntect), and search
  (ripgrep's engine, fd-style file search) are all built in.
- **Read-only & safe.** gandr never mutates your repo — only its own review state under
  `.git/gandr/`.

## Features

**Diff tab — review changes**
- File tree on the left, diff on the right — GitHub-like.
- **Unified or side-by-side** view, toggleable.
- **delta-style rendering**: full-row background for add/remove lines with `+`/`-` signs,
  word-level intra-line highlighting, and multi-line-aware syntax highlighting.
- **Smart-but-explicit comparison**: by default shows your uncommitted changes vs `HEAD`;
  one key (`c`) to compare against a branch, a commit range, or a PR. Optional `--smart`
  auto-selection (uncommitted → branch-vs-base → PR).
- **Fuzzy branch/tag picker** (`b`): fzf-style search over every local & remote ref.
- **Expandable context** (`o`): widen the lines shown around each hunk (3→10→30→100),
  or reveal a single folded gap with `Enter`.
- **Live auto-refresh** as files change on disk.
- **Review tracking**: mark files reviewed; persists per-repo; flags files that changed
  after you reviewed them.
- In-diff search (`/`) that jumps across **all** changed files.

**Repo tab — browse the whole tree**
- Lazy file tree of the entire working tree (only `.git/` is skipped), with a
  syntax-highlighted, line-cursored preview.
- **Repo-wide search** (`/`): file names (fd-style, respects `.gitignore`) or file
  contents (ripgrep's engine) — jump straight to the file and line. No external binaries.

**Everywhere**
- Theme auto-detected from your terminal background (light/dark).
- Open-in-editor (`e` → `$VISUAL`/`$EDITOR` at the current line), help overlay (`?`).
- **Select & copy** (`v` then `y`): grab lines from the preview with a `path:line`
  header — ready to paste back to your agent.
- **Works without git too.** Open gandr in any folder; outside a repo it's a fast,
  read-only file browser (the Diff tab is just empty).

Read-only by design — gandr never modifies your repo (review state lives under
`.git/gandr/`).

## Usage

```bash
gandr                  # uncommitted changes vs HEAD
gandr main             # working tree vs main
gandr main..feature    # a commit range
gandr --staged         # staged changes
gandr --pr             # the current branch's PR (via gh)
gandr --smart          # auto-pick what to compare
```

Press `2` for the **Repo** browser, `1` to go back to the **Diff** reviewer.

### Keys

| Key | Action |
| --- | --- |
| `j`/`k`, `↑`/`↓` | move / scroll · `g`/`G` top/bottom · `Ctrl-d`/`u` half-page |
| `Tab` | switch tree ↔ content focus |
| `n`/`p` | next / previous file · `]`/`[` next / previous hunk |
| `h`/`l` | collapse / expand directory · `Enter` open file / expand fold |
| `s` · `w` · `o` | side-by-side · word highlight · expand context |
| `Space` | mark file reviewed |
| `c` · `b` | compare menu · branch/tag picker |
| `/` (then `n`/`N`) | search · `e` open in `$EDITOR` · `z` hide tree |
| `1` / `2` | Diff / Repo tab · `?` help · `q` quit |

## Install

```bash
# Homebrew (macOS / Linux)
brew install giovaborgogno/tap/gandr

# Cargo (crates.io)
cargo install gandr

# From source
cargo build --release         # binary at target/release/gandr
```
`git` and `gh` (for PR mode) should be on PATH.

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

## License

MIT — see [LICENSE](LICENSE).
