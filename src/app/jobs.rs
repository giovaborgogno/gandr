//! The async backbone: a single event channel the UI loop blocks on, plus
//! helpers to run blocking work (terminal input, diff recompute) on background
//! threads and deliver results as events. Heavy work never blocks the UI; stale
//! results are dropped via an epoch token (see `App`).
//!
//! We use worker threads + crossbeam channels (not an async runtime): git2, the
//! filesystem and the search crates are all blocking, so a thread pool is the
//! right tool — see ADR/PLAN M13.

use crate::diff::engine;
use crate::diff::FileDiff;
use crate::git::git2_backend::Git2Backend;
use crate::git::{CompareSpec, FileChange, GitBackend};
use crate::highlight::{FgSpan, Highlighter, ThemeMode};
use crate::search::{self, SearchMode, SearchResults};
use anyhow::Result;
use crossbeam_channel::Sender;
use crossterm::event::Event as TermEvent;
use ratatui::layout::Size;
use ratatui_image::picker::Picker as ImagePicker;
use ratatui_image::protocol::Protocol;
use ratatui_image::Resize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Identifies the image currently targeted for preview, so a decode result can
/// be matched to the still-selected file (a stale result is dropped). The two
/// consumers — the Diff viewer and the Repo browser — are distinct even for the
/// same path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageKey {
    Diff(PathBuf),
    File(PathBuf),
}

/// Where a decode job reads its bytes from. The Diff side re-fetches the blob
/// via the backend (opened fresh on the worker thread, like [`spawn_diff`],
/// since `git2::Repository` isn't `Send`); the Files side reads from disk.
pub enum ImageSource {
    Diff {
        root: PathBuf,
        spec: CompareSpec,
        change: FileChange,
    },
    File(PathBuf),
}

/// Everything the UI loop reacts to, on one channel.
pub enum AppEvent {
    /// A terminal input event (key, resize, …).
    Term(TermEvent),
    /// A background diff recompute finished (tagged with the requesting epoch).
    DiffReady {
        epoch: u64,
        files: Result<Vec<FileDiff>>,
    },
    /// The working tree changed (from the file watcher).
    FileChanged,
    /// A background repo-wide search finished (tagged with the requesting epoch).
    SearchReady { epoch: u64, results: SearchResults },
    /// A background syntax-highlight of the selected file finished. Per-line
    /// spans for the old and new sides; the UI renders per-line (no carried
    /// state) until this arrives, so navigation never blocks.
    HighlightReady {
        epoch: u64,
        path: PathBuf,
        mode: ThemeMode,
        old: Vec<Vec<FgSpan>>,
        new: Vec<Vec<FgSpan>>,
    },
    /// A background highlight of the Files-tab preview finished.
    BrowserHighlightReady {
        epoch: u64,
        path: PathBuf,
        spans: Vec<Vec<FgSpan>>,
    },
    /// A background image decode+encode finished (M15b). `proto` is `None` if the
    /// bytes couldn't be read/decoded. Tagged with the requesting epoch so a
    /// stale result (the selection moved on) is dropped, and with the cell `area`
    /// it was encoded for so a terminal resize re-encodes. Both the decode and
    /// the (pane-sized) resize+encode run here, off the render thread — rendering
    /// then just re-emits the ready protocol, which is cheap.
    ImageReady {
        epoch: u64,
        key: ImageKey,
        area: Size,
        proto: Option<Protocol>,
    },
}

/// Forward terminal input on a background thread so the UI loop can block on a
/// single channel instead of polling.
///
/// While `paused` is set, the thread does NOT read stdin — it `poll`s so it can
/// observe the flag, but leaves input on the terminal for a child process (e.g.
/// `$EDITOR`). Without this, this thread races the editor for keystrokes, making
/// it impossible to type/quit (nvim feels broken). `poll` returns immediately on
/// real input, so active keypress latency is unaffected; the timeout only bounds
/// how quickly the pause flag is observed.
pub fn spawn_input(tx: Sender<AppEvent>, paused: Arc<AtomicBool>) {
    use std::time::Duration;
    std::thread::spawn(move || loop {
        if paused.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        match crossterm::event::poll(Duration::from_millis(50)) {
            Ok(true) => {
                // Re-check: don't consume input meant for a child started since poll.
                if paused.load(Ordering::Acquire) {
                    continue;
                }
                match crossterm::event::read() {
                    Ok(ev) => {
                        if tx.send(AppEvent::Term(ev)).is_err() {
                            break; // UI loop gone
                        }
                    }
                    Err(_) => break,
                }
            }
            Ok(false) => {} // timeout: loop to re-check the pause flag
            Err(_) => break,
        }
    });
}

/// Recompute the diff for `spec` on a background thread (opening its own backend,
/// since `git2::Repository` is not `Send`), then post the result tagged `epoch`.
pub fn spawn_diff(
    tx: Sender<AppEvent>,
    root: PathBuf,
    spec: CompareSpec,
    context: usize,
    epoch: u64,
) {
    std::thread::spawn(move || {
        let files = Git2Backend::open(&root)
            .and_then(|backend| engine::build_diffs(&backend, &spec, context));
        let _ = tx.send(AppEvent::DiffReady { epoch, files });
    });
}

/// Run a repo-wide search on a background thread, posting results tagged `epoch`.
/// Stale results (a newer query was typed) are dropped by the UI via the epoch.
pub fn spawn_search(
    tx: Sender<AppEvent>,
    root: PathBuf,
    query: String,
    mode: SearchMode,
    epoch: u64,
) {
    std::thread::spawn(move || {
        let results = search::run(&root, &query, mode);
        let _ = tx.send(AppEvent::SearchReady { epoch, results });
    });
}

/// Highlight the selected file's two sides on a background thread (syntect is
/// O(file) and slow on large files), posting the per-line spans tagged `epoch`.
pub fn spawn_highlight(
    tx: Sender<AppEvent>,
    epoch: u64,
    path: PathBuf,
    old_text: String,
    new_text: String,
    mode: ThemeMode,
) {
    std::thread::spawn(move || {
        let hl = Highlighter::for_path(&path, mode);
        let old = hl.highlight_file(&engine::split_lines(&old_text));
        let new = hl.highlight_file(&engine::split_lines(&new_text));
        let _ = tx.send(AppEvent::HighlightReady {
            epoch,
            path,
            mode,
            old,
            new,
        });
    });
}

/// Decode *and* encode the targeted image off-thread — both the O(pixels) decode
/// and the pane-sized resize+encode (the latter is ~20ms full-screen, enough to
/// drop frames if done inline while scrolling). Posts a ready-to-render
/// [`Protocol`] tagged with `epoch` and the `area` it was sized for. `picker` is
/// a cheap clone of the detected protocol/font-size.
pub fn spawn_image(
    tx: Sender<AppEvent>,
    epoch: u64,
    key: ImageKey,
    source: ImageSource,
    picker: ImagePicker,
    area: Size,
) {
    std::thread::spawn(move || {
        let bytes = match source {
            ImageSource::Diff { root, spec, change } => Git2Backend::open(&root)
                .and_then(|backend| backend.file_contents(&spec, &change))
                .ok()
                .and_then(|(old, new)| new.or(old)),
            ImageSource::File(path) => std::fs::read(&path).ok(),
        };
        let proto = bytes
            .as_deref()
            .and_then(crate::image_preview::decode)
            .and_then(|img| picker.new_protocol(img, area, Resize::Fit(None)).ok());
        let _ = tx.send(AppEvent::ImageReady {
            epoch,
            key,
            area,
            proto,
        });
    });
}

/// Highlight the Files-tab preview off-thread (syntect is slow on real code), so
/// moving the cursor over files never blocks.
pub fn spawn_file_highlight(
    tx: Sender<AppEvent>,
    epoch: u64,
    path: PathBuf,
    lines: Vec<String>,
    mode: ThemeMode,
) {
    std::thread::spawn(move || {
        let spans = Highlighter::for_path(&path, mode).highlight_file(&lines);
        let _ = tx.send(AppEvent::BrowserHighlightReady { epoch, path, spans });
    });
}
