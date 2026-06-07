//! A cursor + scroll pair with the shared "follow the cursor" / step logic.
//!
//! Every scrollable pane (the diff body, the diff tree, the repo tree, the file
//! preview) was hand-rolling the same viewport math. This is the one
//! implementation; panes own a `Viewport` and keep their own side effects
//! (loading a file, syncing the diff selection) around these primitives.

use std::cell::Cell;

#[derive(Default)]
pub struct Viewport {
    cursor: usize,
    /// First visible row; follows the cursor, updated at render time (hence the
    /// `Cell`, so `scroll()` can run from an `&self` render path).
    scroll: Cell<usize>,
    /// Visual-selection anchor: `Some` while selecting (the selection spans the
    /// rows between it and the cursor, inclusive). `None` when not selecting.
    anchor: Option<usize>,
}

impl Viewport {
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
    }

    /// Reset to the top (e.g. when a different file is selected). Also clears
    /// any active visual selection — the anchor refers to rows in the old
    /// content and must not persist into the new one.
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.scroll.set(0);
        self.anchor = None;
    }

    /// Pin the viewport to the top (the cursor is set separately).
    pub fn scroll_to_top(&self) {
        self.scroll.set(0);
    }

    /// First visible row so the cursor stays within `height` rows. When `total`
    /// is given, the top is clamped so the last page doesn't scroll past the end.
    pub fn scroll(&self, height: usize, total: Option<usize>) -> usize {
        let mut s = self.scroll.get();
        if self.cursor < s {
            s = self.cursor;
        } else if height > 0 && self.cursor >= s + height {
            s = self.cursor + 1 - height;
        }
        if let Some(total) = total {
            s = s.min(total.saturating_sub(height.max(1)));
        }
        self.scroll.set(s);
        s
    }

    /// After a jump, park the cursor `margin` rows from the top (so the target
    /// and the lines below it are visible, not pinned to the bottom edge).
    pub fn anchor_near_top(&self, margin: usize) {
        self.scroll.set(self.cursor.saturating_sub(margin));
    }

    /// Move the cursor by `n`, clamped to `[0, last]` (diff body / file preview).
    pub fn step_clamped(&mut self, down: bool, n: usize, last: usize) {
        self.cursor = if down {
            (self.cursor + n).min(last)
        } else {
            self.cursor.saturating_sub(n)
        };
    }

    // ---- visual selection ----

    /// Start a selection at the cursor, or clear it if one is active (toggle).
    pub fn toggle_selection(&mut self) {
        self.anchor = match self.anchor {
            Some(_) => None,
            None => Some(self.cursor),
        };
    }

    /// Cancel any active selection.
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// The inclusive `(lo, hi)` row range of the active selection, if any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor
            .map(|a| (a.min(self.cursor), a.max(self.cursor)))
    }

    /// Whether `row` falls inside the active selection.
    pub fn is_selected(&self, row: usize) -> bool {
        self.selection()
            .is_some_and(|(lo, hi)| row >= lo && row <= hi)
    }

    /// Move the cursor one row, wrapping at the ends like a wheel (tree/list).
    pub fn step_wrapping(&mut self, down: bool, len: usize) {
        if len == 0 {
            return;
        }
        self.cursor = if down {
            if self.cursor + 1 >= len {
                0
            } else {
                self.cursor + 1
            }
        } else if self.cursor == 0 {
            len - 1
        } else {
            self.cursor - 1
        };
    }
}
