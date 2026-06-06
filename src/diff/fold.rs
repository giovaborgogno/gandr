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
        let remaining = len - revealed;
        if remaining < MIN_FOLD {
            // Fully (or nearly) revealed — show the whole run inline.
            out.extend((start..i).map(DiffRow::Line));
        } else if i == n {
            // Trailing gap (no change below) → reveal from the top, fold last,
            // so revealed context continues down from the change above.
            out.extend((start..start + revealed).map(DiffRow::Line));
            out.push(DiffRow::Fold {
                anchor: start,
                hidden: remaining,
            });
        } else {
            // A change follows → reveal from the BOTTOM (the lines adjacent to
            // that change), with the fold above them. So expanding a gap reveals
            // context leading into the change you're looking at, not the far top.
            out.push(DiffRow::Fold {
                anchor: start,
                hidden: remaining,
            });
            out.extend((i - revealed..i).map(DiffRow::Line));
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
    fn leading_gap_reveals_from_the_bottom_toward_the_change() {
        let full = sample();
        // The leading gap [0,7) precedes the change → revealing 3 shows the lines
        // nearest the change (4,5,6) with the fold above them.
        let expanded = HashMap::from([(0usize, 3usize)]);
        let rows = fold(&full, 3, &expanded);
        assert_eq!(
            rows[0],
            DiffRow::Fold {
                anchor: 0,
                hidden: 4
            }
        );
        assert_eq!(rows[1], DiffRow::Line(4));
        assert_eq!(rows[2], DiffRow::Line(5));
        assert_eq!(rows[3], DiffRow::Line(6));
        // The trailing fold (no change below) reveals from the top instead.
        let expanded = HashMap::from([(14usize, 2usize)]);
        let rows = fold(&full, 3, &expanded);
        // ...lines 14,15 shown, then the fold for the rest.
        assert!(rows.contains(&DiffRow::Line(14)));
        assert!(rows.contains(&DiffRow::Line(15)));
        assert!(rows.contains(&DiffRow::Fold {
            anchor: 14,
            hidden: 4
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
