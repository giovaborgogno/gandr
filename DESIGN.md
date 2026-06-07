# gandr — Design Spec

> Status: **locked** for v1. Changes require an ADR in `docs/decisions/` and an update here.
> This is the source of truth for *what* gandr is. `PLAN.md` tracks *how/when* it gets built.

## 1. Purpose

`gandr` is a **read-only** terminal UI (ratatui) for reviewing git diffs. It never
mutates the repository. It is optimized for one workflow:

> *I am working with an AI coding agent (e.g. Claude Code). I want to watch the
> changes it makes live, or review the PR it opened — with the clarity of GitHub's
> diff view and the rendering quality of `delta`.*

Non-goals: staging, committing, conflict resolution, posting review comments. It is a
viewer, not a git client.

## 2. CLI

```
gandr                  # default: ALL uncommitted vs HEAD (staged + unstaged together)
gandr <ref>            # working tree vs <ref>            e.g. gandr main
gandr <ref>..<ref>     # commit range
gandr --staged         # index vs HEAD
gandr --pr [N]         # PR via gh (current branch's PR if N omitted)
gandr --smart          # enable smart auto-selection for this run
gandr <path>           # scope the comparison to a path
```

**Smart selection is opt-in only** (`--smart` or `smart_compare = true`). When enabled
and there are no uncommitted changes, it falls back: branch-vs-base (merge-base with the
first matching `base_branches` entry) → PR. The bare `gandr` default is always the plain
uncommitted diff.

## 3. Layout

```
┌─ gandr · feature/x → main · 5 files  +120 −34 ───────────────── ◉ watching ─┐
├───────────────┬──────────────────────────────────────────────────────────────┤
│ Files (5)     │ src/app.rs                                  +12 −4   [2/5] ✓   │
│               │                                                                │
│ ✓ M  app.rs   │   10  10    fn main() {                                        │
│ ▸ A  db.rs    │   11     ▎- let x = 1;                                         │
│   M  ui.rs    │       11 ▎+ let x = 2;                                         │
│ ⚠ M  core/    │   12  12    println!("{x}")                                    │
│      lib.rs   │   ⋯  expand context  ⋯                                         │
├───────────────┴──────────────────────────────────────────────────────────────┤
│ j/k move · Tab focus · n/p file · s split · c compare · Space review · ? help  │
└────────────────────────────────────────────────────────────────────────────────┘
```

- **File tree** (left): compact (single-child dirs collapsed, e.g. `core/lib.rs`),
  expanded by default. Markers: `M/A/D/R` status, `✓` reviewed, `⚠` changed-since-reviewed,
  `▸` cursor.
- **Header**: comparison, file count, totals, watch indicator.
- **File header** (right, sticky): path, per-file +/−, position `[i/n]`, reviewed check.
- **Keybar**: contextual hints.

## 4. Rendering (delta-style)

- Added/removed lines get a **subtle background** color; changed **tokens** within a line
  get a **stronger background** (word-level). Syntax highlighting (syntect) is layered
  underneath. This is the two-layer model `delta` uses.
- Old + new line-number gutters (toggleable). Hunk bar `▎` on the left edge.
- **Unified** and **side-by-side** views (`s`). In side-by-side, long lines **wrap**.
- Collapsible context: show `context_lines` around changes, with an "expand context" affordance.
- Binary / image files render a placeholder ("binary file, N bytes").

## 5. Navigation — **hybrid** focus model

`Tab` switches focus between tree and diff (focus drives `j/k`). **`n/p` (file) and
`] / [` (hunk) always work regardless of focus.**

| Key | Action |
|---|---|
| `j` `k` / `↓` `↑` | scroll diff, or move in tree (whichever is focused) |
| `Tab` | switch focus tree ↔ diff |
| `n` `p` | next / prev **file** (global) |
| `]` `[` | next / prev **hunk** (global) |
| `g` `G` | top / bottom |
| `Ctrl-d` `Ctrl-u` | half-page down / up |
| `Enter` / `→` `←` | expand / collapse tree node |
| `s` | toggle unified ↔ side-by-side |
| `w` | toggle word-level highlight |
| `c` | open compare picker |
| `Space` | mark file reviewed |
| `o` | collapse / expand file or context block |
| `/` | **find in the current view** (diff: whole changeset · Repo: open file); `n`/`N` next/prev match (see ADR 0007) |
| `f` `F` | **finder** (contextual): `f` by file name, `F` by contents. In the Diff tab they search the changed files and jump in place; in the Repo tab they're repo-wide (content → a quickfix list, `n`/`N` walk every match across files). `Tab` toggles mode (see ADR 0007) |
| `e` | open file in `$EDITOR` at the current line |
| `y` | copy path / selection |
| `r` | manual refresh · `a` toggle auto-refresh |
| `?` | help overlay · `q` quit |

Mouse: wheel scroll + click to select a file.

## 6. Behaviors

- **Watch / auto-refresh** (notify + debounce): enabled for working-tree comparisons;
  disabled for static comparisons (ranges, single commits, PRs). On refresh it preserves
  scroll position and the selected file. A subtle "updated" flash signals a refresh.
- **Review state**: persisted in `.git/gandr/state.json`, keyed by comparison spec.
  When a reviewed file changes (e.g. the agent edits it again), keep the `✓` **and** show
  a `⚠ changed since reviewed` badge — never silently lose progress, never hide new changes.
- **Empty state**: "No uncommitted changes. Press `c` to compare against a branch, or run
  with `--smart`."
- **Position carries across tabs**: switching Diff ↔ Repo keeps you on the same file at
  the same line (when it exists on the other side — a Repo file with no changes can't open
  in the diff).
- **Read-only**: gandr never writes to the working tree, index, or refs.

## 7. Theme — `theme = "auto"` (default)

What matters is the **terminal background**, not the OS setting (terminals may differ from
the system theme, or run over SSH). At startup (before entering the alt-screen) gandr queries
the terminal background via **OSC 11** (`termbg`), falling back to `COLORFGBG`, then to dark.

- dark → dark syntect theme + dark diff palette (dim red/green backgrounds)
- light → light syntect theme + light diff palette

```toml
theme = "auto"   # "auto" | "light" | "dark" | "<exact syntect theme name>"
```

A specific theme name disables detection.

## 8. Config

`~/.config/gandr/config.toml`, overridable per-repo by `.gandr.toml`.

```toml
default_view  = "unified"        # | "side-by-side"
smart_compare = false            # smart auto-selection is opt-in
word_diff     = true
auto_refresh  = true
context_lines = 3
tab_width     = 4
theme         = "auto"
editor_cmd    = "code -g {file}:{line}"   # {file} {line} placeholders
base_branches = ["main", "master", "develop"]
# [colors] and [keys] tables override diff palette and keybindings
```

## 9. Backend abstraction

All git access goes through a `GitBackend` trait (see `docs/architecture.md`). v1 ships a
single `git2` implementation. The trait exists so the backend can be swapped for `gix`
(gitoxide) later without touching the diff engine or UI — see `docs/decisions/0001`.
