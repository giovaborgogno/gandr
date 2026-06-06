//! The compact file tree: builds a directory tree from the changed-file paths,
//! collapsing single-child directory chains (VS Code "compact folders"), and
//! flattens it into the visible rows the panel renders and navigates.

use crate::diff::FileDiff;
use crate::git::Status;
use crate::review::ReviewStatus;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// What a visible tree row represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    /// A directory node (possibly a compacted chain like `src/app`).
    Dir { path: PathBuf, expanded: bool },
    /// A changed file, by index into the `files` slice.
    File { index: usize },
}

/// One visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub depth: usize,
    pub label: String,
    pub kind: RowKind,
}

impl Row {
    /// The file index if this row is a file.
    pub fn file_index(&self) -> Option<usize> {
        match self.kind {
            RowKind::File { index } => Some(index),
            RowKind::Dir { .. } => None,
        }
    }
}

#[derive(Default)]
struct Dir {
    subdirs: BTreeMap<String, Dir>,
    files: Vec<(String, usize)>,
}

impl Dir {
    fn insert(&mut self, components: &[String], index: usize) {
        match components {
            [] => {}
            [file] => self.files.push((file.clone(), index)),
            [head, rest @ ..] => {
                self.subdirs
                    .entry(head.clone())
                    .or_default()
                    .insert(rest, index);
            }
        }
    }
}

/// Build the flattened, compacted, collapse-aware visible rows for the file tree.
pub fn build_rows(files: &[FileDiff], collapsed: &HashSet<PathBuf>) -> Vec<Row> {
    let mut root = Dir::default();
    for (index, file) in files.iter().enumerate() {
        let components: Vec<String> = file
            .change
            .path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        root.insert(&components, index);
    }

    let mut rows = Vec::new();
    emit(&root, &PathBuf::new(), 0, collapsed, &mut rows);
    rows
}

fn emit(dir: &Dir, prefix: &Path, depth: usize, collapsed: &HashSet<PathBuf>, out: &mut Vec<Row>) {
    // Directories first (sorted by BTreeMap), then files.
    for (name, sub) in &dir.subdirs {
        // Compact single-child directory chains: `src` → `src/app` → ... .
        let mut label = name.clone();
        let mut full = prefix.join(name);
        let mut cur = sub;
        while cur.files.is_empty() && cur.subdirs.len() == 1 {
            if let Some((child_name, child)) = cur.subdirs.iter().next() {
                label = format!("{label}/{child_name}");
                full = full.join(child_name);
                cur = child;
            } else {
                break;
            }
        }

        let expanded = !collapsed.contains(&full);
        out.push(Row {
            depth,
            label,
            kind: RowKind::Dir {
                path: full.clone(),
                expanded,
            },
        });
        if expanded {
            emit(cur, &full, depth + 1, collapsed, out);
        }
    }
    for (name, index) in &dir.files {
        out.push(Row {
            depth,
            label: name.clone(),
            kind: RowKind::File { index: *index },
        });
    }
}

fn status_marker(files: &[FileDiff], index: usize) -> char {
    files
        .get(index)
        .map(|f| f.change.status.marker())
        .unwrap_or('?')
}

fn marker_color(files: &[FileDiff], index: usize) -> Color {
    match files.get(index).map(|f| f.change.status) {
        Some(Status::Added) => Color::Green,
        Some(Status::Deleted) => Color::Red,
        Some(Status::Renamed) | Some(Status::Copied) => Color::Cyan,
        _ => Color::Yellow,
    }
}

/// Review marker span (✓ reviewed, ⚠ changed-since, blank otherwise).
fn review_span(status: ReviewStatus, row_style: Style) -> Span<'static> {
    match status {
        ReviewStatus::Reviewed => Span::styled("✓ ", row_style.fg(Color::Green)),
        ReviewStatus::ChangedSinceReviewed => Span::styled("⚠ ", row_style.fg(Color::Yellow)),
        ReviewStatus::Unreviewed => Span::styled("  ", row_style),
    }
}

/// Render the file tree into `area`. `cursor` is the selected visible-row index;
/// `scroll` is the first visible row (the panel scrolls to follow the cursor);
/// `statuses` holds the per-file review status, indexed by file index.
#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    files: &[FileDiff],
    rows: &[Row],
    statuses: &[ReviewStatus],
    cursor: usize,
    scroll: usize,
    block: Block,
) {
    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(scroll).take(height) {
        let indent = "  ".repeat(row.depth);
        let selected = i == cursor;
        let row_style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };

        let mut spans = match &row.kind {
            RowKind::Dir { expanded, .. } => {
                let arrow = if *expanded { '▾' } else { '▸' };
                vec![Span::styled(
                    format!("{indent}{arrow} {}/", row.label),
                    row_style.fg(Color::Blue),
                )]
            }
            RowKind::File { index } => {
                let status = statuses
                    .get(*index)
                    .copied()
                    .unwrap_or(ReviewStatus::Unreviewed);
                vec![
                    Span::styled(indent, row_style),
                    review_span(status, row_style),
                    Span::styled(
                        format!("{} ", status_marker(files, *index)),
                        row_style.fg(marker_color(files, *index)),
                    ),
                    Span::styled(row.label.clone(), row_style),
                ]
            }
        };

        // Extend the row to the full panel width so the selected row's highlight
        // spans the whole panel, not just the text.
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let width = inner.width as usize;
        if used < width {
            spans.push(Span::styled(" ".repeat(width - used), row_style));
        }

        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), inner);
}
