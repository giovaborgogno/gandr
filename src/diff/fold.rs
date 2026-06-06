//! Display folding: turn a file's full annotated line list (see
//! [`engine::all_lines`](super::engine::all_lines)) into the rows actually shown
//! — keeping `context` lines around each change and collapsing longer unchanged
//! runs into a single fold marker the user can expand on demand (per-gap expand).

use super::{Line, LineKind};
use std::collections::HashMap;

/// One row of the displayed (folded) diff for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    /// A diff line, by index into the file's full line list.
    Line(usize),
    /// A collapsed run of `hidden` unchanged lines, shown as a single marker.
    /// `anchor` is the run's original start index (a stable key the user's
    /// per-gap reveals are recorded against, even after a partial reveal).
    Fold { anchor: usize, hidden: usize },
}

/// A hidden run shorter than this is shown inline rather than collapsed — a
/// one-line "⋯ 1 unchanged lines ⋯" marker saves no space and just adds noise.
const MIN_FOLD: usize = 2;

/// Fold `full` into display rows: changed lines and any unchanged line within
/// `context` rows of a change stay visible; longer unchanged runs collapse to a
/// [`DiffRow::Fold`].
///
/// `expanded` maps a run's anchor (its original start index) to how many of its
/// lines the user has revealed from the top (per-gap incremental expand). Those
/// lines are shown and the fold shrinks; once the remainder drops below
/// [`MIN_FOLD`] it's shown in full.
pub fn fold(full: &[Line], context: usize, expanded: &HashMap<usize, usize>) -> Vec<DiffRow> {
    let n = full.len();
    if n == 0 {
        return Vec::new();
    }

    // shown[i]: line i is a change, or within `context` rows of one.
    let mut shown = vec![false; n];
    for c in (0..n).filter(|&i| full[i].kind != LineKind::Context) {
        let lo = c.saturating_sub(context);
        let hi = (c + context + 1).min(n);
        for s in &mut shown[lo..hi] {
            *s = true;
        }
    }

    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if shown[i] {
            out.push(DiffRow::Line(i));
            i += 1;
            continue;
        }
        // A maximal run of hidden (unchanged, far-from-change) lines.
        let start = i;
        while i < n && !shown[i] {
            i += 1;
        }
        let len = i - start;
        let revealed = expanded.get(&start).copied().unwrap_or(0).min(len);
        // Reveal `revealed` lines from the top of the run.
        out.extend((start..start + revealed).map(DiffRow::Line));
        let remaining = len - revealed;
        if remaining < MIN_FOLD {
            // Show the (tiny) remainder inline rather than leaving a stub fold.
            out.extend((start + revealed..i).map(DiffRow::Line));
        } else {
            out.push(DiffRow::Fold {
                anchor: start,
                hidden: remaining,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(old: u32) -> Line {
        Line {
            kind: LineKind::Context,
            old_no: Some(old),
            new_no: Some(old),
            text: format!("line {old}"),
            segments: Vec::new(),
        }
    }
    fn add(no: u32) -> Line {
        Line {
            kind: LineKind::Add,
            old_no: None,
            new_no: Some(no),
            text: format!("add {no}"),
            segments: Vec::new(),
        }
    }

    /// 20 context lines with a single change at index 10.
    fn sample() -> Vec<Line> {
        let mut v: Vec<Line> = (0..20).map(ctx).collect();
        v[10] = add(11);
        v
    }

    #[test]
    fn folds_far_context_keeps_near_context() {
        let full = sample();
        let rows = fold(&full, 3, &HashMap::new());
        // Expect: Fold(0..7), lines 7..14 shown (3 ctx + change + 3 ctx), Fold(14..20).
        assert_eq!(
            rows.first(),
            Some(&DiffRow::Fold {
                anchor: 0,
                hidden: 7
            })
        );
        assert_eq!(
            rows.last(),
            Some(&DiffRow::Fold {
                anchor: 14,
                hidden: 6
            })
        );
        let shown: Vec<usize> = rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Line(i) => Some(*i),
                _ => None,
            })
            .collect();
        assert_eq!(shown, vec![7, 8, 9, 10, 11, 12, 13]);
    }

    #[test]
    fn incremental_reveal_from_the_top() {
        let full = sample();
        // Reveal 3 of the leading fold's 7 hidden lines.
        let expanded = HashMap::from([(0usize, 3usize)]);
        let rows = fold(&full, 3, &expanded);
        // Lines 0,1,2 now shown, then a smaller Fold for the remaining 4.
        assert_eq!(rows[0], DiffRow::Line(0));
        assert_eq!(rows[1], DiffRow::Line(1));
        assert_eq!(rows[2], DiffRow::Line(2));
        assert_eq!(
            rows[3],
            DiffRow::Fold {
                anchor: 0,
                hidden: 4
            }
        );
        // The trailing fold is untouched.
        assert!(rows.contains(&DiffRow::Fold {
            anchor: 14,
            hidden: 6
        }));
    }

    #[test]
    fn revealing_enough_removes_the_fold() {
        let full = sample();
        // Revealing 6 of 7 leaves a 1-line remainder (< MIN_FOLD) → shown inline.
        let expanded = HashMap::from([(0usize, 6usize)]);
        let rows = fold(&full, 3, &expanded);
        assert!(!rows
            .iter()
            .any(|r| matches!(r, DiffRow::Fold { anchor: 0, .. })));
        // A reveal larger than the run is clamped (no panic, fully shown).
        let big = HashMap::from([(0usize, 999usize)]);
        let rows = fold(&full, 3, &big);
        assert_eq!(rows[0], DiffRow::Line(0));
    }

    #[test]
    fn no_changes_or_all_changes() {
        // All context, no changes → one big fold (used only if a file had no diff).
        let all_ctx: Vec<Line> = (0..5).map(ctx).collect();
        assert_eq!(
            fold(&all_ctx, 3, &HashMap::new()),
            vec![DiffRow::Fold {
                anchor: 0,
                hidden: 5
            }]
        );
        // All additions (new file) → everything shown, no folds.
        let all_add: Vec<Line> = (1..=4).map(add).collect();
        let rows = fold(&all_add, 3, &HashMap::new());
        assert!(rows.iter().all(|r| matches!(r, DiffRow::Line(_))));
        assert_eq!(rows.len(), 4);
    }
}
