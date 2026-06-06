//! The diff engine: turns raw file contents into the [`FileDiff`] model using
//! `imara-diff`. Line-level hunks with context folding here; intra-line word
//! [`Segment`](super::Segment)s arrive in M3.

use super::{word, FileDiff, Hunk, Line, LineKind};
use crate::git::{CompareSpec, FileChange, GitBackend};
use anyhow::Result;
use imara_diff::{sources::lines, Algorithm, Diff, InternedInput};

/// Whether a byte slice should be treated as binary: not valid UTF-8, or it
/// contains a NUL byte.
fn looks_binary(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_err() || bytes.contains(&0)
}

/// Decode a present side as UTF-8 text (empty for an absent side). Only called
/// once the file is known not to be binary, so decoding cannot lose data.
fn as_text(bytes: Option<&[u8]>) -> &str {
    match bytes {
        Some(b) => std::str::from_utf8(b).unwrap_or(""),
        None => "",
    }
}

/// Strip a single trailing `\n` (and a preceding `\r`) for display.
fn strip_newline(s: &str) -> &str {
    match s.strip_suffix('\n') {
        Some(rest) => rest.strip_suffix('\r').unwrap_or(rest),
        None => s,
    }
}

/// Split `text` into display lines using the same line source as the diff
/// engine, so the resulting `Vec` is indexed identically to the `old_no`/`new_no`
/// (1-based) carried on each [`Line`]. Trailing newlines are stripped for display.
pub fn split_lines(text: &str) -> Vec<String> {
    lines(text).map(|l| strip_newline(l).to_string()).collect()
}

/// Compute displayed line hunks (with `context` lines of surrounding context,
/// adjacent changes merged) for two UTF-8 texts.
pub fn line_hunks(old: &str, new: &str, context: usize) -> Vec<Hunk> {
    let old_lines: Vec<&str> = lines(old).collect();
    let new_lines: Vec<&str> = lines(new).collect();

    let input = InternedInput::new(old, new);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    // Each imara hunk is a contiguous change as (before = old line range,
    // after = new line range). Group changes whose context windows touch.
    let mut groups: Vec<Vec<imara_diff::Hunk>> = Vec::new();
    for h in diff.hunks() {
        if let Some(group) = groups.last_mut() {
            if let Some(prev) = group.last() {
                let gap = h.before.start.saturating_sub(prev.before.end) as usize;
                if gap <= 2 * context {
                    group.push(h);
                    continue;
                }
            }
        }
        groups.push(vec![h]);
    }

    groups
        .iter()
        .filter_map(|g| build_hunk(g, &old_lines, &new_lines, context))
        .collect()
}

/// Build one displayed hunk from a group of adjacent imara change-hunks.
fn build_hunk(
    group: &[imara_diff::Hunk],
    old_lines: &[&str],
    new_lines: &[&str],
    context: usize,
) -> Option<Hunk> {
    let first = group.first()?;
    let last = group.last()?;

    let old_start = (first.before.start as usize).saturating_sub(context);
    let old_end = (last.before.end as usize + context).min(old_lines.len());
    // Context before the first change is 1:1, so the new side starts the same
    // distance back from the change.
    let pre = first.before.start as usize - old_start;
    let new_start = first.after.start as usize - pre;

    let mut out: Vec<Line> = Vec::new();
    let mut old_i = old_start;
    let mut new_i = new_start;
    let mut ci = 0;

    while old_i < old_end || ci < group.len() {
        // A change begins exactly at the current old position?
        if let Some(change) = group.get(ci) {
            if old_i == change.before.start as usize {
                let (ds, de) = (change.before.start as usize, change.before.end as usize);
                let (as_, ae) = (change.after.start as usize, change.after.end as usize);

                let mut del_lines: Vec<Line> = old_lines[ds..de]
                    .iter()
                    .enumerate()
                    .map(|(offset, text)| Line {
                        kind: LineKind::Del,
                        old_no: Some((ds + offset) as u32 + 1),
                        new_no: None,
                        text: strip_newline(text).to_string(),
                        segments: Vec::new(),
                    })
                    .collect();
                let mut add_lines: Vec<Line> = new_lines[as_..ae]
                    .iter()
                    .enumerate()
                    .map(|(offset, text)| Line {
                        kind: LineKind::Add,
                        old_no: None,
                        new_no: Some((as_ + offset) as u32 + 1),
                        text: strip_newline(text).to_string(),
                        segments: Vec::new(),
                    })
                    .collect();

                // Word-level emphasis for removed/added lines paired by position.
                for (del, add) in del_lines.iter_mut().zip(add_lines.iter_mut()) {
                    let (old_segs, new_segs) = word::segments(&del.text, &add.text);
                    del.segments = old_segs;
                    add.segments = new_segs;
                }

                out.extend(del_lines);
                out.extend(add_lines);
                old_i = de;
                new_i = ae;
                ci += 1;
                continue;
            }
        }

        // Otherwise an unchanged context line (1:1 on both sides).
        if old_i < old_end {
            out.push(Line {
                kind: LineKind::Context,
                old_no: Some(old_i as u32 + 1),
                new_no: Some(new_i as u32 + 1),
                text: strip_newline(old_lines[old_i]).to_string(),
                segments: Vec::new(),
            });
            old_i += 1;
            new_i += 1;
        } else {
            break;
        }
    }

    let old_count = old_i - old_start;
    let new_count = new_i - new_start;
    Some(Hunk {
        old_start: old_start as u32 + 1,
        new_start: new_start as u32 + 1,
        header: format!(
            "@@ -{},{} +{},{} @@",
            old_start + 1,
            old_count,
            new_start + 1,
            new_count
        ),
        lines: out,
    })
}

/// Build a [`FileDiff`] from one file's old/new contents. Non-UTF-8 content on a
/// present side marks the file binary (no hunks). Recomputes +/- counts from the
/// produced hunks for display consistency.
pub fn build_file_diff(
    mut change: FileChange,
    old: Option<&[u8]>,
    new: Option<&[u8]>,
    context: usize,
) -> FileDiff {
    // A side is binary if it isn't valid UTF-8 or contains a NUL byte (NUL is
    // valid UTF-8 but a strong binary signal, mirroring git's heuristic).
    if old.is_some_and(looks_binary) || new.is_some_and(looks_binary) {
        change.is_binary = true;
        change.additions = 0;
        change.deletions = 0;
        return FileDiff {
            change,
            hunks: Vec::new(),
            old_text: String::new(),
            new_text: String::new(),
        };
    }

    // Not binary ⇒ both present sides are valid UTF-8.
    let old_text = as_text(old);
    let new_text = as_text(new);
    let hunks = line_hunks(old_text, new_text, context);

    let mut additions = 0;
    let mut deletions = 0;
    for hunk in &hunks {
        for line in &hunk.lines {
            match line.kind {
                LineKind::Add => additions += 1,
                LineKind::Del => deletions += 1,
                LineKind::Context => {}
            }
        }
    }
    change.additions = additions;
    change.deletions = deletions;

    FileDiff {
        change,
        hunks,
        old_text: old_text.to_string(),
        new_text: new_text.to_string(),
    }
}

/// Compute diffs for every changed file in a comparison, via the backend.
pub fn build_diffs(
    backend: &dyn GitBackend,
    spec: &CompareSpec,
    context: usize,
) -> Result<Vec<FileDiff>> {
    let changes = backend.changed_files(spec)?;
    let mut out = Vec::with_capacity(changes.len());
    for change in changes {
        let (old, new) = backend.file_contents(spec, &change)?;
        out.push(build_file_diff(
            change,
            old.as_deref(),
            new.as_deref(),
            context,
        ));
    }
    Ok(out)
}
