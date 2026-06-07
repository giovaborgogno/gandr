//! The "Files" tab: a lazy file browser over the whole working tree (including
//! git-ignored files/folders — only `.git/` is skipped), plus the content of the
//! selected file. State lives here; rendering is in `ui::browser`.

use crate::highlight::{FgSpan, ThemeMode};
use crate::ui::viewport::Viewport;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Don't read files larger than this into the content preview.
const MAX_PREVIEW_BYTES: u64 = 2_000_000;

/// Rows from the top to park the preview cursor after a search/reveal jump.
const JUMP_MARGIN: usize = 3;

/// Files up to this many lines get whole-file (multi-line aware) highlighting at
/// load; longer files are highlighted per visible line at render time instead, so
/// selecting them stays instant. ~1200 lines is well under a perceptible hitch.
pub const HL_MAX_LINES: usize = 1200;

/// What a visible browser row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Dir { expanded: bool },
    File,
}

/// A visible row in the file browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserRow {
    pub depth: usize,
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
}

/// The loaded content of the selected file (cached so we don't read every frame).
pub struct Loaded {
    pub path: PathBuf,
    pub lines: Vec<String>,
    /// Syntax highlight spans per line, computed with carried state across the
    /// whole file (M12) so multi-line constructs render correctly. One entry per
    /// line in `lines` (empty for binary/too-large files).
    pub highlights: Vec<Vec<FgSpan>>,
    pub binary: bool,
    pub too_large: bool,
}

/// File-browser state for the Files tab.
pub struct Browser {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    /// Cursor + scroll over the tree's visible rows.
    tree: Viewport,
    /// Cursor + scroll over the preview's lines (the cursor row is highlighted).
    content: Viewport,
    loaded: Option<Loaded>,
    /// Cache of visible rows (rebuilt only when the expanded set changes, not
    /// every frame — avoids walking disk on every render/keystroke). Handed out
    /// as a cheap `Rc` clone so navigation never copies the whole tree.
    rows_cache: RefCell<Rc<Vec<BrowserRow>>>,
    rows_dirty: Cell<bool>,
    /// Theme mode for syntax highlighting (resolved by the app at startup).
    mode: ThemeMode,
}

/// List a directory's children: directories first then files, alphabetical,
/// skipping `.git`. Returns `(name, path, is_dir)`.
fn read_dir_sorted(dir: &Path) -> Vec<(String, PathBuf, bool)> {
    let mut entries: Vec<(String, PathBuf, bool)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    return None;
                }
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                Some((name, e.path(), is_dir))
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    entries.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0))); // dirs first, then name
    entries
}

impl Browser {
    pub fn new(root: PathBuf) -> Self {
        let mut browser = Self {
            root,
            expanded: HashSet::new(),
            tree: Viewport::default(),
            content: Viewport::default(),
            loaded: None,
            rows_cache: RefCell::new(Rc::new(Vec::new())),
            rows_dirty: Cell::new(true),
            mode: ThemeMode::Dark,
        };
        browser.load_selection();
        browser
    }

    /// Set the theme mode and drop the loaded file's highlights so they're
    /// recomputed (asynchronously) for the new theme.
    pub fn set_mode(&mut self, mode: ThemeMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if let Some(loaded) = &mut self.loaded {
            loaded.highlights = Vec::new();
        }
    }

    pub fn mode(&self) -> ThemeMode {
        self.mode
    }

    /// The loaded file's (path, lines) if it still needs highlighting — its spans
    /// are empty and it's a previewable text file under [`HL_MAX_LINES`].
    /// syntect is slow (~100ms+ for a few hundred lines of real code), so this is
    /// done off-thread; the preview renders plain until the result lands.
    pub fn highlight_target(&self) -> Option<(PathBuf, Vec<String>)> {
        let l = self.loaded.as_ref()?;
        if l.binary || l.too_large || !l.highlights.is_empty() || l.lines.len() > HL_MAX_LINES {
            return None;
        }
        Some((l.path.clone(), l.lines.clone()))
    }

    /// Apply async highlight spans to the loaded file (ignored if it changed).
    pub fn apply_highlights(&mut self, path: &Path, spans: Vec<Vec<FgSpan>>) {
        if let Some(l) = &mut self.loaded {
            if l.path == path {
                l.highlights = spans;
            }
        }
    }

    /// Visible rows (cached; rebuilt from disk only when the expanded set
    /// changed). Returns a cheap `Rc` handle — no whole-tree clone per call.
    pub fn rows(&self) -> Rc<Vec<BrowserRow>> {
        if self.rows_dirty.get() {
            let mut out = Vec::new();
            self.emit(&self.root, 0, &mut out);
            *self.rows_cache.borrow_mut() = Rc::new(out);
            self.rows_dirty.set(false);
        }
        Rc::clone(&self.rows_cache.borrow())
    }

    /// Number of visible rows (O(1) — never clones the tree).
    pub fn rows_len(&self) -> usize {
        self.rows().len()
    }

    /// One row by index — clones a single row, not the whole tree.
    fn row_at(&self, idx: usize) -> Option<BrowserRow> {
        self.rows().get(idx).cloned()
    }

    fn invalidate(&self) {
        self.rows_dirty.set(true);
    }

    fn emit(&self, dir: &Path, depth: usize, out: &mut Vec<BrowserRow>) {
        for (name, path, is_dir) in read_dir_sorted(dir) {
            if is_dir {
                let expanded = self.expanded.contains(&path);
                out.push(BrowserRow {
                    depth,
                    name,
                    path: path.clone(),
                    kind: EntryKind::Dir { expanded },
                });
                if expanded {
                    self.emit(&path, depth + 1, out);
                }
            } else {
                out.push(BrowserRow {
                    depth,
                    name,
                    path,
                    kind: EntryKind::File,
                });
            }
        }
    }

    pub fn cursor(&self) -> usize {
        self.tree.cursor()
    }

    /// Line indices (0-based) in the loaded file containing `query` (smart-case:
    /// case-insensitive unless the query has an uppercase char). For n/N in the
    /// content preview after a repo content-search.
    pub fn match_lines(&self, query: &str) -> Vec<usize> {
        let Some(loaded) = &self.loaded else {
            return Vec::new();
        };
        let sensitive = query.chars().any(|c| c.is_uppercase());
        let needle = if sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        loaded
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                if sensitive {
                    line.contains(&needle)
                } else {
                    line.to_lowercase().contains(&needle)
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Put the preview cursor on `line` (0-based) — e.g. a search match — and
    /// park it a few rows from the top.
    pub fn scroll_content_to(&mut self, line: usize) {
        self.content.set_cursor(line.min(self.content_last_line()));
        self.content.anchor_near_top(JUMP_MARGIN);
    }

    /// Last valid preview line index.
    fn content_last_line(&self) -> usize {
        self.loaded
            .as_ref()
            .map(|l| l.lines.len().saturating_sub(1))
            .unwrap_or(0)
    }

    /// First visible tree row so the cursor stays on-screen for `height` rows.
    pub fn tree_scroll(&self, height: usize) -> usize {
        self.tree.scroll(height, None)
    }

    pub fn loaded(&self) -> Option<&Loaded> {
        self.loaded.as_ref()
    }
    /// The current preview line (for cursor highlighting + search position).
    pub fn content_cursor(&self) -> usize {
        self.content.cursor()
    }
    /// Viewport top of the preview, keeping the cursor visible for `height` rows.
    pub fn content_scroll(&self, height: usize) -> usize {
        let total = self.loaded.as_ref().map(|l| l.lines.len()).unwrap_or(0);
        self.content.scroll(height, Some(total))
    }
    // ---- navigation ----

    pub fn cursor_down(&mut self) {
        self.tree.step_wrapping(true, self.rows_len());
        self.load_selection();
    }

    pub fn cursor_up(&mut self) {
        self.tree.step_wrapping(false, self.rows_len());
        self.load_selection();
    }

    /// Enter/→: expand a directory, or (on a file) does nothing extra (already loaded).
    pub fn expand_or_open(&mut self) {
        if let Some(row) = self.row_at(self.tree.cursor()) {
            if let EntryKind::Dir { .. } = row.kind {
                self.expanded.insert(row.path);
                self.invalidate();
            }
        }
    }

    /// Whether the cursor is on a directory row.
    pub fn cursor_is_dir(&self) -> bool {
        matches!(
            self.row_at(self.tree.cursor()).map(|r| r.kind),
            Some(EntryKind::Dir { .. })
        )
    }

    /// Enter on a directory: expand if collapsed, collapse if expanded.
    pub fn toggle(&mut self) {
        if let Some(row) = self.row_at(self.tree.cursor()) {
            if let EntryKind::Dir { expanded } = row.kind {
                if expanded {
                    self.expanded.remove(&row.path);
                } else {
                    self.expanded.insert(row.path);
                }
                self.invalidate();
            }
        }
        let len = self.rows_len();
        if len > 0 && self.tree.cursor() >= len {
            self.tree.set_cursor(len - 1);
        }
    }

    /// ←: collapse the directory under the cursor.
    pub fn collapse(&mut self) {
        if let Some(row) = self.row_at(self.tree.cursor()) {
            if let EntryKind::Dir { expanded: true, .. } = row.kind {
                self.expanded.remove(&row.path);
                self.invalidate();
            }
        }
        let len = self.rows_len();
        if len > 0 && self.tree.cursor() >= len {
            self.tree.set_cursor(len - 1);
        }
    }

    /// Reveal `path` (an absolute path under the root): expand its ancestor
    /// directories, move the cursor onto it, load it, and — for a content hit —
    /// scroll the preview so `line` (1-based) sits near the top. Used to jump to
    /// a repo-search result.
    pub fn reveal(&mut self, path: &Path, line: Option<usize>) {
        let mut dir = path.parent();
        while let Some(d) = dir {
            if d == self.root {
                break;
            }
            if d.starts_with(&self.root) {
                self.expanded.insert(d.to_path_buf());
            }
            dir = d.parent();
        }
        self.invalidate();

        if let Some(idx) = self.rows().iter().position(|r| r.path == path) {
            self.tree.set_cursor(idx);
            self.load_selection();
            // `load_selection` resets the scroll; place the match near the top,
            // clamped to the file we actually loaded (only after a successful
            // load, so a missing row never scrolls the previously-shown file).
            if let Some(l) = line {
                self.content
                    .set_cursor(l.saturating_sub(1).min(self.content_last_line()));
                self.content.anchor_near_top(JUMP_MARGIN);
            }
        }
    }

    pub fn scroll_content_down(&mut self, n: usize) {
        self.content.step_clamped(true, n, self.content_last_line());
    }
    pub fn scroll_content_up(&mut self, n: usize) {
        self.content
            .step_clamped(false, n, self.content_last_line());
    }

    /// Read the file under the cursor into the content cache (if it changed).
    fn load_selection(&mut self) {
        let Some(row) = self.row_at(self.tree.cursor()) else {
            return;
        };
        if row.kind != EntryKind::File {
            return; // keep the last opened file shown while on a directory
        }
        if self.loaded.as_ref().map(|l| &l.path) == Some(&row.path) {
            return;
        }
        self.content.reset();

        // Don't read huge files into memory for a preview.
        if std::fs::metadata(&row.path).map(|m| m.len()).unwrap_or(0) > MAX_PREVIEW_BYTES {
            self.loaded = Some(Loaded {
                path: row.path,
                lines: Vec::new(),
                highlights: Vec::new(),
                binary: false,
                too_large: true,
            });
            return;
        }

        let loaded = match std::fs::read(&row.path) {
            Ok(bytes) => {
                let binary = bytes.contains(&0) || std::str::from_utf8(&bytes).is_err();
                let lines = if binary {
                    Vec::new()
                } else {
                    String::from_utf8_lossy(&bytes)
                        .lines()
                        .map(str::to_string)
                        .collect()
                };
                Loaded {
                    path: row.path,
                    lines,
                    highlights: Vec::new(),
                    binary,
                    too_large: false,
                }
            }
            Err(_) => Loaded {
                path: row.path,
                lines: Vec::new(),
                highlights: Vec::new(),
                binary: false,
                too_large: false,
            },
        };
        // Highlights are computed off-thread (see `highlight_target`); the preview
        // renders plain until they land, so selecting a file never blocks.
        self.loaded = Some(loaded);
    }
}
