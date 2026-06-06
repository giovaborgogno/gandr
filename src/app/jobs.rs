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
use anyhow::Result;
use crossbeam_channel::Sender;
use crossterm::event::Event as TermEvent;
use std::path::PathBuf;

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
}

/// Forward terminal input on a background thread so the UI loop can block on a
/// single channel instead of polling.
pub fn spawn_input(tx: Sender<AppEvent>) {
    std::thread::spawn(move || {
        while let Ok(ev) = crossterm::event::read() {
            if tx.send(AppEvent::Term(ev)).is_err() {
                break; // UI loop gone
            }
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
