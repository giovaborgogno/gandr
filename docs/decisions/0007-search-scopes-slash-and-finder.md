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
2. **`f` / `F` = the repo-wide finder**, available from **both** tabs: `f` opens it by
   file name, `F` by contents; `Tab` toggles the mode while open. A jump lands in the
   Repo preview at the match; `n`/`N` then steps that file (unchanged behavior).
3. **One search active at a time ("last wins").** Opening `/` closes the finder and
   vice-versa, so `n`/`N` is never ambiguous.

## Consequences
- `/` finally means one thing everywhere; the powerful finder becomes a global navigator
  (a natural fit if the two tabs ever merge into one view).
- Repo-wide *content* stepping after a finder jump stays in-file for now (matches the
  prior behavior). A future cross-file "quickfix walk" (step every match across all files,
  nvim `:cnext`-style) is left as a follow-up; it would not change these key bindings.
- Supersedes the `/` row of the DESIGN.md keymap (updated alongside this ADR).
