//! gdiff — a read-only TUI for reviewing git diffs.
//!
//! Layered, one-directional dependencies (see `docs/architecture.md`):
//! `app` → `ui`/`highlight` → `diff` → `git` (the `GitBackend` trait).
//! Nothing depends back upward; the UI never touches `git2` directly.

pub mod app;
pub mod cli;
pub mod config;
pub mod diff;
pub mod git;
pub mod highlight;
pub mod ui;
