//! Filesystem watcher for live auto-refresh of working-tree comparisons.
//!
//! Watches the repo root (recursively, debounced) and signals on a channel when
//! non-`.git` files change. The `.git` directory is ignored to avoid refresh
//! storms from git's own writes (and gandr's own `.git/gandr/state.json`).

use anyhow::Result;
use crossbeam_channel::Receiver;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::Path;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(400);

/// A running watcher. Holds the debouncer alive; drop it to stop watching.
pub struct Watcher {
    _debouncer: Debouncer<RecommendedWatcher>,
    /// Receives a unit message (debounced) whenever relevant files change.
    pub rx: Receiver<()>,
}

/// Start watching `root` recursively. Errors (e.g. an unwatchable path) are the
/// caller's to tolerate — gandr simply runs without auto-refresh.
pub fn watch(root: &Path) -> Result<Watcher> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let mut debouncer = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| {
        if let Ok(events) = res {
            // Ignore changes entirely inside .git (git internals + our state file).
            let relevant = events
                .iter()
                .any(|e| !e.path.components().any(|c| c.as_os_str() == ".git"));
            if relevant {
                let _ = tx.send(());
            }
        }
    })?;
    debouncer.watcher().watch(root, RecursiveMode::Recursive)?;
    Ok(Watcher {
        _debouncer: debouncer,
        rx,
    })
}
