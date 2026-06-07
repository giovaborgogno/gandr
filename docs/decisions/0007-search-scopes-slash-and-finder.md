# 0007 — Search scopes: `/` finds in the view, `f`/`F` find in the repo

## Context
The two tabs had drifted on search. `/` meant different things per tab: in the Diff
tab it searched the whole diff (vim-style, `n`/`N` across changed files); in the Repo
tab it opened a repo-wide *finder* overlay (fzf-style: file-name ⇄ content via `Tab`,
a results list, jump-to). So the same key did two unrelated things depending on where
you were — the opposite of what every mature tool does.

Surveying the standard (vim/less, VS Code, yazi/ranger, lazygit, nvim+telescope): the
convention is unanimous — **`/` always means "find in the thing I'm looking at"**, and
"find anything in the project" lives on a *separate* trigger (VS Code `Cmd+P`/`Cmd+Shift+F`,
yazi `s`/`S`, nvim telescope/`:grep`+quickfix). `/` never changes meaning by context.

## Decision
1. **`/` = find in the current view**, `n`/`N` walk that view's matches:
   - Diff tab → the whole changeset (all changed files), as before.
   - Repo tab → the open preview file (in-file), live highlight + `n`/`N`.
   - Rule: `/` searches the view's *natural corpus*; the corpus differs because the
     views differ (the Diff view *is* the multi-file changeset; the Repo preview is one
     file). Commit (`Enter`) lands on the first match at-or-after the cursor; `n`/`N`
     step strictly after/before, wrapping.
2. **The finder is `f` (by file name) / `F` (by contents); `Tab` toggles the mode.** Both
   are **contextual**: in the Diff tab they search only the *changed files* and jump within
   the diff (never leaving the tab); in the Repo tab they're repo-wide and land in the Repo
   preview.
3. **Match-stepping after a jump matches the corpus.** A repo-wide content (`F`) jump opens
   a quickfix list — `n`/`N` step **every match across the whole repo**, crossing files
   (the nvim `:grep` → `:cnext` model); the keybar shows `[i/n] across repo`, `Esc` closes
   it. A diff-scoped content jump instead arms the in-view `/` search, so `n`/`N` walk every
   match across the changed files.
4. **One search active at a time ("last wins").** Opening `/`, `f`, or `F` clears the
   others (the in-view find, the finder overlay, and the quickfix list), so `n`/`N` is
   never ambiguous.

## Consequences
- `/` means one thing everywhere; `f`/`F` are the project-wide navigator. `f` is scoped to
  the view's natural corpus (changed files in the Diff, the whole tree in the Repo), the
  same principle that governs `/`.
- The quickfix list is the third piece of search state (alongside the in-view `/` and the
  finder overlay); each entry point clears the other two to keep `n`/`N` unambiguous.
- Supersedes the `/` and finder rows of the DESIGN.md keymap (updated alongside this ADR).
