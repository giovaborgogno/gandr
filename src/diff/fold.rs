//! Display folding: turn a file's full annotated line list (see
//! [`engine::all_lines`](super::engine::all_lines)) into the rows actually shown
//! — keeping `context` lines around each change and collapsing longer unchanged
//! runs into a single fold marker the user can expand on demand (per-gap expand).

use super::{Line, LineKind};
use std::collections::HashSet;

/// One row of the displayed (folded) diff for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    /// A diff line, by index into the file's full line list.
    Line(usize),
    /// A collapsed run of unchanged lines `[start, start + hidden)` (indices into
    /// the full line list), shown as a single "⋯ N lines ⋯" marker. `start` is a
    /// stable anchor used to remember which folds the user expanded.
    Fold { start: usize, hidden: usize },
}

/// A hidden run shorter than this is shown inline rather than collapsed — a
/// one-line "⋯ 1 unchanged lines ⋯" marker saves no space and just adds noise.
const MIN_FOLD: usize = 2;

/// Fold `full` into display rows: changed lines and any unchanged line within
/// `context` rows of a change stay visible; longer unchanged runs collapse to a
/// [`DiffRow::Fold`] — unless the run's `start` anchor is in `expanded` (per-gap
/// expand) or the run is shorter than [`MIN_FOLD`], in which case it's shown.
pub fn fold(full: &[Line], context: usize, expanded: &HashSet<usize>) -> Vec<DiffRow> {
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
        if expanded.contains(&start) || i - start < MIN_FOLD {
            out.extend((start..i).map(DiffRow::Line));
        } else {
            out.push(DiffRow::Fold {
                start,
                hidden: i - start,
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
        let rows = fold(&full, 3, &HashSet::new());
        // Expect: Fold(0..7), lines 7..14 shown (3 ctx + change + 3 ctx), Fold(14..20).
        assert_eq!(
            rows.first(),
            Some(&DiffRow::Fold {
                start: 0,
                hidden: 7
            })
        );
        assert_eq!(
            rows.last(),
            Some(&DiffRow::Fold {
                start: 14,
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
    fn expanding_a_fold_reveals_its_lines() {
        let full = sample();
        let mut expanded = HashSet::new();
        expanded.insert(0); // expand the leading fold
        let rows = fold(&full, 3, &expanded);
        // The leading fold's lines 0..7 are now shown; the trailing fold remains.
        assert_eq!(rows.first(), Some(&DiffRow::Line(0)));
        assert!(rows.contains(&DiffRow::Fold {
            start: 14,
            hidden: 6
        }));
        assert!(!rows
            .iter()
            .any(|r| matches!(r, DiffRow::Fold { start: 0, .. })));
    }

    #[test]
    fn no_changes_or_all_changes() {
        // All context, no changes → one big fold (used only if a file had no diff).
        let all_ctx: Vec<Line> = (0..5).map(ctx).collect();
        assert_eq!(
            fold(&all_ctx, 3, &HashSet::new()),
            vec![DiffRow::Fold {
                start: 0,
                hidden: 5
            }]
        );
        // All additions (new file) → everything shown, no folds.
        let all_add: Vec<Line> = (1..=4).map(add).collect();
        let rows = fold(&all_add, 3, &HashSet::new());
        assert!(rows.iter().all(|r| matches!(r, DiffRow::Line(_))));
        assert_eq!(rows.len(), 4);
    }
}
