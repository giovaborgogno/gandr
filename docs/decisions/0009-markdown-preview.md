# 0009 — Rendered Markdown preview via `tui-markdown`

## Context
Agents write a lot of Markdown (READMEs, plans, ADRs, changelogs). Reviewing it as
highlighted source works for the diff, but reading the *document* — tables, lists,
alerts, code blocks — is much easier rendered. We want a toggle between the source view
and a rendered preview, in both the Diff tab and the Repo browser.

Options considered:
1. **Embed `md-tui`** (a terminal Markdown viewer). It does expose a library, but it is
   built as an application: rendering reads process-global config/theme state from
   `~/.config/mdt` (`LazyLock` statics), which makes output depend on the user's machine
   (non-deterministic snapshots), and it pulls a large dependency tree (tree-sitter + a
   grammar per language, pest, `config`, `mermaid-text`, its own `ratatui-image` setup).
   Its widgets own layout and navigation, which would fight our viewer/scroll model.
2. **`tui-markdown`** — a small library that converts Markdown (pulldown-cmark, GFM) into a
   ratatui `Text`, with a pluggable `StyleSheet` and a table-width option. Pure function,
   no global state, depends only on `ratatui-core` 0.1 (= our ratatui 0.30), pulldown-cmark,
   itertools, tracing.
3. Hand-roll a pulldown-cmark → `Line` renderer.

## Decision
1. Use **`tui-markdown`** with `default-features = false`. Its `highlight-code` feature
   enables syntect with `regex-onig` (a C dependency we deliberately avoid — see AGENTS.md);
   instead we keep the fence delimiters, read the info-string language, drop the fences,
   and highlight code lines with our own `Highlighter` (`for_token`).
2. A thin adapter (`src/ui/markdown.rs`, render-pure) owns the gandr look: a light/dark
   `StyleSheet`, word wrapping with hanging indents (tui-markdown leaves wrapping to the
   consumer), typographic list/task markers, heading underline rules, full-width rules, and
   a padded code background. GFM alert icons are suppressed (emoji with variation selectors
   have ambiguous terminal widths and corrupt the row).
3. `m` toggles the mode; it is sticky across Markdown files. The Diff tab previews the new
   version (old for deletions). Rendered lines are cached per (file, content hash, width,
   theme), so per-frame cost is a hash + slice.

## Consequences
- New deps: `tui-markdown` (+ pulldown-cmark), `unicode-width` (already transitive).
- The preview is a read-only document view: no cursor, selection, or `/` search inside it
  (j/k/g/G/Ctrl-d/u scroll it). Those, and source ↔ preview position mapping, are follow-ups.
- Images referenced from Markdown show as `[img] alt` text for now.
- DESIGN.md §4/§5 updated alongside this ADR.
