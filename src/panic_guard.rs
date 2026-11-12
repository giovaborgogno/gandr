//! Contain panics from third-party code run on the UI thread.
//!
//! `catch_unwind` alone isn't enough inside the TUI: the panic hook still runs,
//! and the one `ratatui::try_init` installs restores the terminal (leaves the
//! alternate screen) before the unwind is caught — the app would keep running
//! on a wrecked screen. [`install_filter`] wraps the current hook so it stays
//! silent for panics raised inside [`contain`] on the same thread; every other
//! panic reaches the original hook unchanged.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};

thread_local! {
    static CONTAINED: Cell<bool> = const { Cell::new(false) };
}

/// Wrap the current panic hook with the [`contain`] filter. Call it after each
/// `ratatui::try_init` (which installs its own restoring hook on top).
pub fn install_filter() {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if !CONTAINED.with(Cell::get) {
            prev(info);
        }
    }));
}

/// Run `f`, turning a panic into `None` (silently, once [`install_filter`] ran).
pub fn contain<T>(f: impl FnOnce() -> T) -> Option<T> {
    let was = CONTAINED.with(|c| c.replace(true));
    let out = panic::catch_unwind(AssertUnwindSafe(f)).ok();
    CONTAINED.with(|c| c.set(was));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contain_turns_panics_into_none() {
        assert_eq!(contain(|| 7), Some(7));
        assert_eq!(contain(|| -> i32 { panic!("boom") }), None);
        // The flag is reset afterwards.
        assert!(!CONTAINED.with(Cell::get));
    }
}
