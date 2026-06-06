//! gdiff — a read-only TUI for reviewing git diffs.
//!
//! Layered, one-directional dependencies (see `docs/architecture.md`):
//! `app` → `ui`/`highlight` → `diff` → `git` (the `GitBackend` trait).
//! Nothing depends back upward; the UI never touches `git2` directly.

pub mod app;
pub mod browser;
pub mod cli;
pub mod config;
pub mod diff;
pub mod fuzzy;
pub mod git;
pub mod highlight;
pub mod review;
pub mod search;
pub mod testutil;
pub mod ui;
