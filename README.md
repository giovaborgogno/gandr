# gandr

**Review code changes and browse your repo, from the terminal.** A read-only TUI with the
clarity of GitHub's diff view and the rendering quality of
[`delta`](https://github.com/dandavison/delta). Built for one workflow: watching the changes
an AI coding agent (like Claude Code) makes — live as it edits, or as the PR it opened — then
exploring the rest of the codebase without leaving your terminal.

![gandr in action](https://github.com/user-attachments/assets/766c2fca-513c-4b36-9702-00eb565a49c9)

> Status: feature-complete and stable. Two tabs — a **Diff** reviewer and a
> **Repo** browser — with live refresh, review tracking, and repo-wide search.
> A few DX extras are backlogged (config-file loading, copy, mouse) — see `PLAN.md`.

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
cargo install gandr           # from crates.io
# or, from a clone:
cargo build --release         # binary at target/release/gandr
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

## License

MIT — see [LICENSE](LICENSE).
