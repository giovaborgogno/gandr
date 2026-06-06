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
use crate::git::CompareSpec;
use crate::highlight::{FgSpan, Highlighter, ThemeMode};
use crate::search::{self, SearchMode, SearchResults};
use anyhow::Result;
use crossbeam_channel::Sender;
use crossterm::event::Event as TermEvent;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
