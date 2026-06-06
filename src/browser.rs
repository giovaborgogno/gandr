//! The "Files" tab: a lazy file browser over the whole working tree (including
//! git-ignored files/folders — only `.git/` is skipped), plus the content of the
//! selected file. State lives here; rendering is in `ui::browser`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Don't read files larger than this into the content preview.
const MAX_PREVIEW_BYTES: u64 = 2_000_000;

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
    pub binary: bool,
    pub too_large: bool,
}

/// File-browser state for the Files tab.
pub struct Browser {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    cursor: usize,
    tree_scroll: Cell<usize>,
    content_scroll: usize,
    content_viewport: Cell<usize>,
    loaded: Option<Loaded>,
    /// Cache of visible rows (rebuilt only when the expanded set changes, not
    /// every frame — avoids walking disk on every render/keystroke).
    rows_cache: RefCell<Vec<BrowserRow>>,
    rows_dirty: Cell<bool>,
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
            cursor: 0,
            tree_scroll: Cell::new(0),
            content_scroll: 0,
            content_viewport: Cell::new(0),
            loaded: None,
            rows_cache: RefCell::new(Vec::new()),
            rows_dirty: Cell::new(true),
        };
        browser.load_selection();
        browser
    }

    /// Visible rows (cached; rebuilt from disk only when the expanded set changed).
    pub fn rows(&self) -> Vec<BrowserRow> {
        if self.rows_dirty.get() {
            let mut out = Vec::new();
            self.emit(&self.root, 0, &mut out);
            *self.rows_cache.borrow_mut() = out;
            self.rows_dirty.set(false);
        }
        self.rows_cache.borrow().clone()
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
        self.cursor
    }

    /// First visible tree row so the cursor stays on-screen for `height` rows.
    pub fn tree_scroll(&self, height: usize) -> usize {
        let mut s = self.tree_scroll.get();
        if self.cursor < s {
            s = self.cursor;
        } else if height > 0 && self.cursor >= s + height {
            s = self.cursor + 1 - height;
        }
        self.tree_scroll.set(s);
        s
    }

    pub fn loaded(&self) -> Option<&Loaded> {
        self.loaded.as_ref()
    }
    pub fn content_scroll(&self) -> usize {
        self.content_scroll
    }
    pub fn set_content_viewport(&self, rows: usize) {
        self.content_viewport.set(rows);
    }

    // ---- navigation ----

    pub fn cursor_down(&mut self) {
        let len = self.rows().len();
        if len == 0 {
            return;
        }
        self.cursor = (self.cursor + 1).min(len - 1);
        self.load_selection();
    }

    pub fn cursor_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.load_selection();
    }

    /// Enter/→: expand a directory, or (on a file) does nothing extra (already loaded).
    pub fn expand_or_open(&mut self) {
        if let Some(row) = self.rows().into_iter().nth(self.cursor) {
            if let EntryKind::Dir { .. } = row.kind {
                self.expanded.insert(row.path);
                self.invalidate();
            }
        }
    }

    /// Whether the cursor is on a directory row.
    pub fn cursor_is_dir(&self) -> bool {
        matches!(
            self.rows().into_iter().nth(self.cursor).map(|r| r.kind),
            Some(EntryKind::Dir { .. })
        )
    }

    /// Enter on a directory: expand if collapsed, collapse if expanded.
    pub fn toggle(&mut self) {
        if let Some(row) = self.rows().into_iter().nth(self.cursor) {
            if let EntryKind::Dir { expanded } = row.kind {
                if expanded {
                    self.expanded.remove(&row.path);
                } else {
                    self.expanded.insert(row.path);
                }
                self.invalidate();
            }
        }
        let len = self.rows().len();
        if len > 0 && self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    /// ←: collapse the directory under the cursor.
    pub fn collapse(&mut self) {
        if let Some(row) = self.rows().into_iter().nth(self.cursor) {
            if let EntryKind::Dir { expanded: true, .. } = row.kind {
                self.expanded.remove(&row.path);
                self.invalidate();
            }
        }
        let len = self.rows().len();
        if len > 0 && self.cursor >= len {
            self.cursor = len - 1;
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
            self.cursor = idx;
            self.load_selection();
            // `load_selection` resets the scroll; place the match near the top,
            // clamped to the file we actually loaded (only after a successful
            // load, so a missing row never scrolls the previously-shown file).
            if let Some(l) = line {
                let max = self
                    .loaded
                    .as_ref()
                    .map(|f| f.lines.len().saturating_sub(1))
                    .unwrap_or(0);
                self.content_scroll = l.saturating_sub(1).min(max);
            }
        }
    }

    pub fn scroll_content_down(&mut self, n: usize) {
        let max = self
            .loaded
            .as_ref()
            .map(|l| l.lines.len())
            .unwrap_or(0)
            .saturating_sub(self.content_viewport.get().max(1));
        self.content_scroll = (self.content_scroll + n).min(max);
    }
    pub fn scroll_content_up(&mut self, n: usize) {
        self.content_scroll = self.content_scroll.saturating_sub(n);
    }

    /// Read the file under the cursor into the content cache (if it changed).
    fn load_selection(&mut self) {
        let Some(row) = self.rows().into_iter().nth(self.cursor) else {
            return;
        };
        if row.kind != EntryKind::File {
            return; // keep the last opened file shown while on a directory
        }
        if self.loaded.as_ref().map(|l| &l.path) == Some(&row.path) {
            return;
        }
        self.content_scroll = 0;

        // Don't read huge files into memory for a preview.
        if std::fs::metadata(&row.path).map(|m| m.len()).unwrap_or(0) > MAX_PREVIEW_BYTES {
            self.loaded = Some(Loaded {
                path: row.path,
                lines: Vec::new(),
                binary: false,
                too_large: true,
            });
            return;
        }

        self.loaded = Some(match std::fs::read(&row.path) {
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
                    binary,
                    too_large: false,
                }
            }
            Err(_) => Loaded {
                path: row.path,
                lines: Vec::new(),
                binary: false,
                too_large: false,
            },
        });
    }
}
