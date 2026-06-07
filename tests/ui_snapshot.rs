//! Snapshot tests: drive the real `App`, render to `TestBackend`, golden the frame.
//! This is gandr's deterministic headless "e2e" (see docs/testing.md).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gandr::app::App;
use gandr::config::Config;
use gandr::git::git2_backend::Git2Backend;
use gandr::git::CompareSpec;
use gandr::testutil::Fixture;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;

/// Build an `App` showing the uncommitted diff of a fixture.
fn app_from(fx: &Fixture) -> App {
    let backend = Box::new(Git2Backend::open(fx.path()).unwrap());
    App::new(Config::default(), backend, CompareSpec::Uncommitted).unwrap()
}

/// Render the App to a text frame of the given size. The header shows the branch
/// and comparison (not the random temp-dir path), so frames are deterministic.
fn frame(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| app.render(f)).unwrap();
    terminal.backend().to_string()
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty())
}

/// Legend char for a cell background, matching the default [`Palette`].
fn bg_legend(c: Color) -> char {
    match c {
        Color::Rgb(20, 48, 28) => 'a',  // add background
        Color::Rgb(36, 94, 52) => 'A',  // add, word-level changed
        Color::Rgb(58, 26, 28) => 'd',  // del background
        Color::Rgb(110, 40, 44) => 'D', // del, word-level changed
        Color::Reset => '.',
        _ => '?',
    }
}

/// Render to a buffer and return (glyphs, background-legend) blocks.
fn render_buffer(app: &App, width: u16, height: u16) -> (String, String) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let completed = terminal.draw(|f| app.render(f)).unwrap();
    let buf = completed.buffer;
    let mut glyphs = String::new();
    let mut bgs = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = buf.cell((x, y)).expect("cell in bounds");
            glyphs.push_str(cell.symbol());
            bgs.push(bg_legend(cell.bg));
        }
        glyphs.push('\n');
        bgs.push('\n');
    }
    (glyphs, bgs)
}

/// A style-aware frame: glyphs plus a background-color map (verifies the
/// delta-style add/del backgrounds and word-level emphasis headlessly).
fn styled_frame(app: &App, width: u16, height: u16) -> String {
    let (glyphs, bgs) = render_buffer(app, width, height);
    format!("{glyphs}── backgrounds ──\n{bgs}")
}

#[test]
fn empty_repo_shows_placeholder() {
    let fx = Fixture::new();
    fx.write("a.txt", "unchanged\n");
    fx.commit("init");
    let app = app_from(&fx);
    insta::assert_snapshot!(frame(&app, 80, 12));
}

#[test]
fn unified_single_file_modify() {
    let fx = Fixture::new();
    fx.write("src/lib.rs", "fn one() {}\nfn two() {}\nfn three() {}\n");
    fx.commit("init");
    fx.write("src/lib.rs", "fn one() {}\nfn TWO() {}\nfn three() {}\n");
    let app = app_from(&fx);
    insta::assert_snapshot!(frame(&app, 80, 14));
}

#[test]
fn multi_file_list_with_first_selected() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("init");
    fx.write("a.txt", "a changed\n");
    fx.write("b.txt", "b changed\n");
    fx.write("c_new.txt", "new file\n");
    let app = app_from(&fx);
    insta::assert_snapshot!(frame(&app, 80, 16));
}

#[test]
fn navigation_n_selects_second_file() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("z.txt", "z\n");
    fx.commit("init");
    fx.write("a.txt", "a changed\n");
    fx.write("z.txt", "z changed\n");
    let mut app = app_from(&fx);
    app.handle_key(key('n')); // select the second file
    insta::assert_snapshot!(frame(&app, 80, 14));
}

#[test]
fn scroll_down_in_diff_keeps_sticky_header() {
    let fx = Fixture::new();
    let original: String = (1..=40).map(|n| format!("line{n}\n")).collect();
    fx.write("big.txt", &original);
    fx.commit("init");
    // Touch lines spread across the file so there are several hunks to scroll.
    let edited = original
        .replace("line5\n", "LINE5\n")
        .replace("line20\n", "LINE20\n")
        .replace("line35\n", "LINE35\n");
    fx.write("big.txt", &edited);

    let mut app = app_from(&fx);
    let _ = frame(&app, 80, 14); // first render sets the viewport height
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())); // focus the diff
    app.handle_key(key(']')); // jump to the next hunk
    insta::assert_snapshot!(frame(&app, 80, 14));
}

// ---- M3: delta-style rendering ----

#[test]
fn m3_word_diff_backgrounds() {
    let fx = Fixture::new();
    fx.write("a.txt", "the quick brown fox\n");
    fx.commit("init");
    fx.write("a.txt", "the quick red fox\n");
    let app = app_from(&fx);
    // The del/add lines get d/a backgrounds; the changed word ("brown"→"red")
    // gets the stronger D/A background.
    insta::assert_snapshot!(styled_frame(&app, 60, 10));
}

#[test]
fn m3_word_diff_toggle_off_removes_strong_bg() {
    let fx = Fixture::new();
    fx.write("a.txt", "the quick brown fox\n");
    fx.commit("init");
    fx.write("a.txt", "the quick red fox\n");
    let mut app = app_from(&fx);
    app.handle_key(key('w')); // disable word-level emphasis
    let (_, bgs) = render_buffer(&app, 60, 10);
    assert!(
        !bgs.contains('A') && !bgs.contains('D'),
        "word emphasis disabled, but found strong backgrounds:\n{bgs}"
    );
    // Plain add/del backgrounds remain.
    assert!(bgs.contains('a') && bgs.contains('d'));
}

#[test]
fn m3_syntax_highlighting_sets_foreground() {
    let fx = Fixture::new();
    fx.write("lib.rs", "fn main() {}\n");
    fx.commit("init");
    fx.write("lib.rs", "fn main() { let x = 42; }\n");
    let mut app = app_from(&fx);
    warm_highlight(&mut app); // syntax highlighting is async; drive the job

    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
    let completed = terminal.draw(|f| app.render(f)).unwrap();
    let buf = completed.buffer;
    // syntect emits Rgb foreground colors; if any text cell is Rgb, highlighting ran.
    let has_rgb_fg = (0..buf.area.height).any(|y| {
        (0..buf.area.width).any(|x| matches!(buf.cell((x, y)).unwrap().fg, Color::Rgb(_, _, _)))
    });
    assert!(
        has_rgb_fg,
        "expected syntect to set at least one Rgb foreground"
    );
}

// ---- M4: tree + side-by-side ----

#[test]
fn tree_shows_compacted_directories() {
    let fx = Fixture::new();
    fx.write("src/app/mod.rs", "fn run() {}\n");
    fx.write("README.md", "# x\n");
    fx.commit("init");
    fx.write("src/app/mod.rs", "fn run() { go(); }\n");
    fx.write("README.md", "# y\n");
    let app = app_from(&fx);
    insta::assert_snapshot!(frame(&app, 80, 12));
}

#[test]
fn side_by_side_view_shows_two_columns() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo\nthree\n");
    fx.commit("init");
    fx.write("a.txt", "one\nTWO\nthree\n");
    let mut app = app_from(&fx);
    app.handle_key(key('s')); // switch to side-by-side
    insta::assert_snapshot!(frame(&app, 80, 12));
}

// ---- M6: review state + refresh ----

#[test]
fn reviewing_a_file_shows_check_in_tree() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "y\n");
    let mut app = app_from(&fx);
    app.handle_key(key(' ')); // mark the selected file reviewed
    insta::assert_snapshot!(frame(&app, 70, 10));
}

#[test]
fn changed_after_review_is_flagged() {
    use gandr::review::ReviewStatus;
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "y\n");
    let mut app = app_from(&fx);

    app.handle_key(key(' ')); // review (diff is x→y)
    assert_eq!(app.review_statuses(), vec![ReviewStatus::Reviewed]);

    fx.write("a.txt", "z\n"); // the file changes again on disk
    app.refresh();
    assert_eq!(
        app.review_statuses(),
        vec![ReviewStatus::ChangedSinceReviewed]
    );
}

#[test]
fn refresh_preserves_selected_file() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("z.txt", "z\n");
    fx.commit("init");
    fx.write("a.txt", "a1\n");
    fx.write("z.txt", "z1\n");
    let mut app = app_from(&fx);
    app.handle_key(key('n')); // select z.txt (second file)
    let before = app.selected();
    app.refresh();
    assert_eq!(app.selected(), before);
}

// ---- M7: search, help, theme ----

#[test]
fn search_jumps_to_match() {
    let fx = Fixture::new();
    let original: String = (1..=40).map(|n| format!("line{n}\n")).collect();
    fx.write("big.txt", &original);
    fx.commit("init");
    fx.write("big.txt", &original.replace("line30\n", "LINE30\n"));

    let mut app = app_from(&fx);
    let _ = frame(&app, 80, 14); // set viewport
    app.handle_key(key('/'));
    for c in "LINE30".chars() {
        app.handle_key(key(c));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
    insta::assert_snapshot!(frame(&app, 80, 14));
}

#[test]
fn diff_search_crosses_files() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.write("z.txt", "x\n");
    fx.commit("init");
    // Both files change to contain the same term.
    fx.write("a.txt", "needle\n");
    fx.write("z.txt", "needle\n");
    let mut app = app_from(&fx);
    let _ = frame(&app, 80, 14);

    app.handle_key(key('/'));
    for c in "needle".chars() {
        app.handle_key(key(c));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
    let first = app.current().unwrap().change.path.clone();
    // The first file has a single match → n crosses into the other file.
    app.handle_key(key('n'));
    let second = app.current().unwrap().change.path.clone();
    assert_ne!(first, second, "n should cross to the next file's match");
    // n again wraps back to the first file.
    app.handle_key(key('n'));
    assert_eq!(app.current().unwrap().change.path, first);
}

#[test]
fn search_highlights_matches() {
    let fx = Fixture::new();
    fx.write("a.txt", "alpha\n");
    fx.commit("init");
    fx.write("a.txt", "alpha beta\n");
    let mut app = app_from(&fx);
    app.handle_key(key('/'));
    for c in "beta".chars() {
        app.handle_key(key(c));
    }
    // Matched text gets a yellow background.
    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
    let completed = terminal.draw(|f| app.render(f)).unwrap();
    let buf = completed.buffer;
    let found = (0..buf.area.height)
        .any(|y| (0..buf.area.width).any(|x| buf.cell((x, y)).unwrap().bg == Color::Yellow));
    assert!(found, "expected a yellow search-match background");
}

#[test]
fn help_overlay() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "y\n");
    let mut app = app_from(&fx);
    app.handle_key(key('?'));
    // Height 20 keeps the centered help popup off row 0, so the tab bar (and its
    // redacted repo path) stays visible — otherwise the popup hides "Files [2]"
    // and the random temp-dir path leaks into the snapshot (non-deterministic).
    insta::assert_snapshot!(frame(&app, 72, 20));
}

#[test]
fn overlays_on_tiny_terminal_do_not_panic() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "y\n");
    let mut app = app_from(&fx);
    app.handle_key(key('?')); // help overlay
    let _ = frame(&app, 8, 4); // absurdly small; must not panic
    app.handle_key(key('?')); // close help
    app.handle_key(key('c')); // compare picker
    let _ = frame(&app, 8, 4);
}

#[test]
fn light_theme_uses_light_backgrounds() {
    use gandr::highlight::ThemeMode;
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "y\n");
    let mut app = app_from(&fx);
    app.set_theme_mode(ThemeMode::Light);

    let mut terminal = Terminal::new(TestBackend::new(60, 10)).unwrap();
    let completed = terminal.draw(|f| app.render(f)).unwrap();
    let buf = completed.buffer;
    // The light palette's add background (see Palette::for_mode(Light)).
    let light_add = Color::Rgb(214, 247, 220);
    let found = (0..buf.area.height)
        .any(|y| (0..buf.area.width).any(|x| buf.cell((x, y)).unwrap().bg == light_add));
    assert!(found, "expected light-mode add background somewhere");
}

// ---- M13: async refresh (epoch supersession) ----

#[test]
fn async_refresh_ignores_stale_results() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "y\n");
    let mut app = app_from(&fx);
    assert!(!app.files().is_empty());

    app.request_refresh();
    assert!(app.is_loading());

    // A result from a superseded (older) epoch is ignored.
    app.apply_diff_result(0, Ok(vec![]));
    assert!(app.is_loading());
    assert!(
        !app.files().is_empty(),
        "stale result must not replace files"
    );

    // The current epoch's result is applied.
    let (epoch, _spec) = app.take_pending_refresh().unwrap();
    app.apply_diff_result(epoch, Ok(vec![]));
    assert!(!app.is_loading());
    assert!(app.files().is_empty());
}

// ---- M8: tabs + files browser ----

#[test]
fn files_tab_browser() {
    let fx = Fixture::new();
    fx.write("src/lib.rs", "pub fn run() {\n    let x = 1;\n}\n");
    fx.write("README.md", "# demo\n");
    fx.commit("init");
    fx.write("src/lib.rs", "pub fn run() {\n    let x = 2;\n}\n");
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // switch to Files tab
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())); // expand first dir (src)
    insta::assert_snapshot!(frame(&app, 90, 14));
}

// ---- M14: repo-wide search (Files tab) ----

/// Drive the async repo-search to completion synchronously (there's no run_loop
/// in tests): take the queued job, run it, and apply the result — the same flow
/// the event loop performs (see `async_refresh_ignores_stale_results`).
fn run_pending_search(app: &mut App, root: &std::path::Path) {
    if let Some((epoch, query, mode)) = app.take_pending_search() {
        let results = gandr::search::run(root, &query, mode);
        app.apply_search_result(epoch, results);
    }
}

#[test]
fn repo_search_content_lists_matches() {
    let fx = Fixture::new();
    fx.write("src/lib.rs", "pub fn greet() {}\n");
    fx.write("src/main.rs", "fn main() {\n    greet();\n}\n");
    fx.commit("init");
    let mut app = app_from(&fx);
    let root = app.context().root.clone();

    app.handle_key(key('2')); // Files tab
    app.handle_key(key('F')); // open the repo-wide finder in content mode
    for c in "greet".chars() {
        app.handle_key(key(c));
    }
    run_pending_search(&mut app, &root);
    insta::assert_snapshot!(frame(&app, 90, 18));
}

#[test]
fn repo_search_files_mode_lists_paths() {
    let fx = Fixture::new();
    fx.write("src/lib.rs", "x\n");
    fx.write("src/main.rs", "y\n");
    fx.write("README.md", "z\n");
    fx.commit("init");
    let mut app = app_from(&fx);
    let root = app.context().root.clone();

    app.handle_key(key('2')); // Files tab
    app.handle_key(key('F')); // open the finder (content mode)
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())); // Tab → file-name mode
    for c in "rs".chars() {
        app.handle_key(key(c));
    }
    run_pending_search(&mut app, &root);
    insta::assert_snapshot!(frame(&app, 90, 18));
}

#[test]
fn repo_search_jumps_to_result() {
    let fx = Fixture::new();
    // The match is on line 3 so the jump must scroll there (not just open the file).
    fx.write("src/lib.rs", "// a\n// b\npub fn greet() {}\n");
    fx.write("src/main.rs", "fn main() {\n    greet();\n}\n");
    fx.commit("init");
    let mut app = app_from(&fx);
    let root = app.context().root.clone();

    app.handle_key(key('2'));
    app.handle_key(key('F')); // repo-wide content finder
    for c in "greet".chars() {
        app.handle_key(key(c));
    }
    run_pending_search(&mut app, &root);
    // Enter jumps to the first result, closing the overlay and revealing the file.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
    assert!(app.repo_search().is_none(), "overlay closes on jump");
    let loaded = app.browser().loaded().expect("a file is revealed");
    assert!(loaded.path.ends_with("lib.rs"));
    // Revealed at the matched line (line 3 → 0-based scroll 2), clamped to file.
    assert_eq!(app.browser().content_cursor(), 2);
}

#[test]
fn finder_content_jump_keeps_query_and_navigates() {
    let fx = Fixture::new();
    // "needle" on lines 1 and 4.
    fx.write(
        "a.rs",
        "let needle = 1;\nlet b = 2;\nlet c = 3;\nlet needle2 = 4;\n",
    );
    fx.commit("init");
    let mut app = app_from(&fx);
    let root = app.context().root.clone();

    app.handle_key(key('2')); // Files
    app.handle_key(key('F')); // repo-wide content finder
    for c in "needle".chars() {
        app.handle_key(key(c));
    }
    run_pending_search(&mut app, &root);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())); // jump to first hit

    // The query is kept so the preview highlights it and n/N can navigate.
    assert_eq!(app.browser_query(), Some("needle"));
    assert_eq!(app.browser().content_cursor(), 0); // first match: line 1
    app.handle_key(key('n')); // → next match (line 4 → 0-based 3)
    assert_eq!(app.browser().content_cursor(), 3);
    app.handle_key(key('n')); // wraps back to the first
    assert_eq!(app.browser().content_cursor(), 0);
    // Switching to the Diff tab clears the preview highlight.
    app.handle_key(key('1'));
    assert_eq!(app.browser_query(), None);
}

#[test]
fn slash_searches_the_open_preview_file_in_the_repo_tab() {
    // `/` in the Repo tab finds within the open file (in-view), not the repo —
    // live highlight + n/N within the file. The repo-wide finder is f/F.
    let fx = Fixture::new();
    fx.write(
        "a.rs",
        "let needle = 1;\nlet b = 2;\nlet c = 3;\nlet needle2 = 4;\n",
    );
    fx.commit("init");
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // Files tab
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty())); // open a.rs preview
    app.handle_key(key('/')); // in-file find
    assert!(
        app.repo_search().is_none(),
        "/ must not open the repo finder"
    );
    for c in "needle".chars() {
        app.handle_key(key(c));
    }
    // Live highlight tracks the query while typing.
    assert_eq!(app.browser_query(), Some("needle"));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())); // jump to first match
    assert_eq!(app.browser().content_cursor(), 0);
    app.handle_key(key('n')); // next match within the file
    assert_eq!(app.browser().content_cursor(), 3);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty())); // Esc clears the find
    assert_eq!(app.browser_query(), None);
}

#[test]
fn finder_opens_in_file_or_content_mode_and_tab_toggles() {
    use gandr::search::SearchMode;
    let fx = Fixture::new();
    fx.write("a.rs", "x\n");
    fx.commit("init");
    let mut app = app_from(&fx); // Diff tab — the finder is global
    app.handle_key(key('f')); // file-name mode
    assert_eq!(
        app.repo_search().map(|rs| rs.mode),
        Some(SearchMode::Files),
        "f opens the finder in file-name mode"
    );
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())); // Tab → content
    assert_eq!(
        app.repo_search().map(|rs| rs.mode),
        Some(SearchMode::Content)
    );
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
    app.handle_key(key('F')); // content mode directly
    assert_eq!(
        app.repo_search().map(|rs| rs.mode),
        Some(SearchMode::Content)
    );
}

// ---- M12 (diff viewer): multi-line syntax highlighting ----

/// Drive the async highlight job to completion (no run_loop in tests), the same
/// flow the event loop performs on selection.
fn warm_highlight(app: &mut App) {
    if let Some((epoch, path, old, new, mode)) = app.take_pending_highlight() {
        let hl = gandr::highlight::Highlighter::for_path(&path, mode);
        let o = hl.highlight_file(&gandr::diff::engine::split_lines(&old));
        let n = hl.highlight_file(&gandr::diff::engine::split_lines(&new));
        app.apply_highlight(epoch, path, mode, o, n);
    }
}

#[test]
fn diff_highlight_carries_state_across_lines() {
    let fx = Fixture::new();
    // A block comment, then a change below it. The comment opener may fold out of
    // view, but the new-side highlight must still treat lines 2–3 as comment.
    fx.write("a.rs", "/* open\n still comment\n */\nlet x = 1;\n");
    fx.commit("init");
    fx.write("a.rs", "/* open\n still comment\n */\nlet x = 2;\n");
    let mut app = app_from(&fx);

    assert!(
        app.diff_highlight().is_none(),
        "highlight is async — not ready until the job runs"
    );
    warm_highlight(&mut app);
    let (_old, new_hl) = app.diff_highlight().expect("ready after the job");
    let opener = new_hl[0].first().map(|s| s.color);
    let interior = new_hl[1].first().map(|s| s.color);
    assert!(opener.is_some());
    assert_eq!(
        opener, interior,
        "an interior comment line must share the opener's color (state carried)"
    );
    // Cached: a second call returns the same handle (Rc) without recomputing.
    let (_o2, new2) = app.diff_highlight().unwrap();
    assert!(std::rc::Rc::ptr_eq(&new_hl, &new2));
}

#[test]
fn diff_highlight_invalidates_on_same_path_edit() {
    let fx = Fixture::new();
    fx.write("a.rs", "let x = 1;\n");
    fx.commit("init");
    fx.write("a.rs", "let x = 2;\n");
    let mut app = app_from(&fx);

    warm_highlight(&mut app);
    let (_o, before) = app.diff_highlight().unwrap();
    // A working-tree edit changes the same file's content (and grows it). The
    // cache key is (path, theme); without invalidation it would serve stale spans.
    fx.write("a.rs", "// added a comment line\nlet x = 3;\nlet y = 4;\n");
    app.refresh(); // re-diffs the same path → apply_files must drop the cache
    assert!(
        app.diff_highlight().is_none(),
        "the edit must invalidate the cached highlight (stale content)"
    );
    warm_highlight(&mut app);
    let (_o2, after) = app.diff_highlight().unwrap();

    assert!(
        !std::rc::Rc::ptr_eq(&before, &after),
        "same-path content change must recompute the highlight, not reuse stale spans"
    );
    // The new map covers the grown file (3 new-side lines).
    assert_eq!(after.len(), 3);
}

#[test]
fn unified_wraps_long_lines() {
    let fx = Fixture::new();
    fx.write("a.txt", "short\n");
    fx.commit("init");
    // A long added line whose tail would be lost to truncation.
    fx.write("a.txt", &format!("{}END\n", "x".repeat(100)));
    let app = app_from(&fx);
    let out = frame(&app, 90, 16);
    assert!(
        out.contains("END"),
        "long line should wrap so its tail stays visible, not truncate:\n{out}"
    );
}

// ---- tree navigation & hide toggle ----

#[test]
fn z_hides_the_tree_panel() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "y\n");
    let mut app = app_from(&fx);
    assert!(
        frame(&app, 80, 10).contains("Changes"),
        "tree shown by default"
    );
    app.handle_key(key('z'));
    assert!(
        !frame(&app, 80, 10).contains("Changes"),
        "z should hide the Changes/tree panel"
    );
    app.handle_key(key('z'));
    assert!(frame(&app, 80, 10).contains("Changes"), "z toggles it back");
}

#[test]
fn h_l_collapse_and_expand_dirs_in_files_tab() {
    let fx = Fixture::new();
    fx.write("src/lib.rs", "pub fn f() {}\n");
    fx.commit("init");
    fx.write("a.txt", "x\n");
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // Files tab; cursor on first row (a dir, e.g. src)
                              // `l` expands the dir under the cursor, `h` collapses it — same as →/←.
    app.handle_key(key('l'));
    let expanded = frame(&app, 90, 14);
    app.handle_key(key('h'));
    let collapsed = frame(&app, 90, 14);
    assert_ne!(
        expanded, collapsed,
        "h/l should collapse/expand a directory in the Files tree"
    );
}

#[test]
fn repo_tab_marks_changed_files() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.commit("init");
    fx.write("a.txt", "two\n"); // modified
    fx.write("b.txt", "new\n"); // added
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // Files tab
    let out = frame(&app, 60, 14);
    // the tree rows carry a colored M (modified) / A (added) marker in the gutter
    assert!(
        out.contains("M   a.txt"),
        "a.txt should show an M marker in the tree:\n{out}"
    );
    assert!(
        out.contains("A   b.txt"),
        "b.txt should show an A marker in the tree:\n{out}"
    );
}

#[test]
fn opens_a_non_git_directory_in_files_only_mode() {
    use gandr::git::null_backend::NullBackend;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hi\n").unwrap();
    let backend = Box::new(NullBackend::new(dir.path().to_path_buf()));
    let mut app = App::new(Config::default(), backend, CompareSpec::Uncommitted)
        .expect("a non-git directory should open");
    app.set_files_only();
    assert!(app.files_only());
    // Starts on the Repo browser, which lists the directory's files.
    let out = frame(&app, 80, 12);
    assert!(
        out.contains("hello.txt"),
        "the Repo tree should list the file:\n{out}"
    );
}

#[test]
fn visual_select_copies_preview_lines_with_context() {
    let fx = Fixture::new();
    fx.write("note.txt", "alpha\nbeta\ngamma\n");
    fx.commit("init");
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // Files tab; cursor on note.txt
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty())); // enter content
    app.handle_key(key('v')); // start selection at line 1
    app.handle_key(key('j')); // extend to line 2
    app.handle_key(key('y')); // copy
    let text = app
        .take_clipboard_request()
        .expect("`y` should queue clipboard text");
    assert!(text.contains("note.txt:1-2"), "header missing in:\n{text}");
    assert!(
        text.contains("alpha") && text.contains("beta") && !text.contains("gamma"),
        "should copy only the selected lines:\n{text}"
    );
}

#[test]
fn arrows_enter_and_exit_a_file_in_diff_tab() {
    // The Diff tab must mirror the Repo tab: l/→ on a file enters its diff,
    // h/← from the diff exits back to the tree (Tab still toggles too).
    use gandr::app::Focus;
    let fx = Fixture::new();
    fx.write("a.txt", "hello\n");
    fx.commit("init");
    fx.write("a.txt", "hello world\n");
    let mut app = app_from(&fx); // starts on the Diff tab, tree focused
    assert_eq!(app.focus(), Focus::Tree, "starts focused on the tree");
    app.handle_key(key('l')); // l on a file enters its diff
    assert_eq!(app.focus(), Focus::Diff, "l on a file enters its diff");
    app.handle_key(key('h')); // h from the diff exits back to the tree
    assert_eq!(
        app.focus(),
        Focus::Tree,
        "h from the diff exits to the tree"
    );
}

#[test]
fn arrows_enter_and_exit_a_file_in_files_tab() {
    use gandr::app::Focus;
    let fx = Fixture::new();
    fx.write("a.txt", "hello\n");
    fx.commit("init");
    fx.write("a.txt", "hello world\n");
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // Files tab; cursor on a.txt (a top-level file)
    assert_eq!(app.focus(), Focus::Tree, "starts focused on the tree");
    let right = KeyEvent::new(KeyCode::Right, KeyModifiers::empty());
    let left = KeyEvent::new(KeyCode::Left, KeyModifiers::empty());
    app.handle_key(right); // → on a file enters its content pane
    assert_eq!(app.focus(), Focus::Diff, "→ on a file enters its content");
    app.handle_key(left); // ← from the content exits back to the tree
    assert_eq!(app.focus(), Focus::Tree, "← from content exits to the tree");
}

#[test]
fn long_lines_wrap_in_the_repo_preview() {
    // The Repo preview must wrap long lines (like the Diff viewer), not truncate
    // them — so the tail of a long line is still visible further down the pane.
    let fx = Fixture::new();
    let long = "word ".repeat(40); // ~200 cols, far wider than any pane here
    fx.write("long.txt", &format!("{long}\nshort\n"));
    fx.commit("init");
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // Files tab
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty())); // enter content
    let out = frame(&app, 80, 16);
    // The line wraps onto multiple rows, so "short" (the next logical line) is
    // pushed well below row 1 and is still rendered — not clipped by truncation.
    assert!(
        out.contains("short"),
        "the second line should still render after the first wraps:\n{out}"
    );
    // A wrapped continuation row carries a blank line-number gutter, so the word
    // count visible exceeds what a single truncated row could hold.
    let words = out.matches("word").count();
    assert!(
        words > 10,
        "a wrapped line should show many segments, got {words}:\n{out}"
    );
}

#[test]
fn preview_keeps_the_cursor_visible_when_lines_wrap() {
    // With wrapped lines each logical line spans several terminal rows, so a
    // naive logical-line scroll would push the cursor (on the last line) off the
    // bottom. The preview must follow the cursor in display rows, like the diff.
    let fx = Fixture::new();
    let body: String = (1..=8)
        .map(|i| format!("L{i} {}\n", "x".repeat(120)))
        .collect();
    fx.write("wide.txt", &body);
    fx.commit("init");
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // Files tab
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty())); // enter content
                                                                          // Ctrl-d jumps the cursor toward the bottom (clamped to the last line).
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    let out = frame(&app, 80, 10);
    assert!(
        out.contains("L8"),
        "the cursor line (L8) must stay visible after wrapping pushes it down:\n{out}"
    );
}

#[test]
fn tree_nav_wraps_around() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("init");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");
    let mut app = app_from(&fx); // focus starts on the tree; two file rows
    assert_eq!(app.tree_cursor(), 0);
    app.handle_key(key('k')); // up at the top wraps to the bottom
    assert_eq!(app.tree_cursor(), 1);
    app.handle_key(key('j')); // down at the bottom wraps to the top
    assert_eq!(app.tree_cursor(), 0);
}

// ---- edge cases: render without panic ----

#[test]
fn untracked_embedded_repo_dir_does_not_crash_startup() {
    // A vendored folder that is itself a git repo (e.g. installed skills under
    // `.claude/skills/<name>/`) — git surfaces it as a single untracked
    // *directory* delta. gandr must skip it, not try to read a directory as a
    // file and take the whole app down at startup.
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    let vend = fx.path().join("vendored");
    std::fs::create_dir_all(&vend).unwrap();
    std::fs::write(vend.join("note.txt"), "hi\n").unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&vend)
        .status()
        .unwrap();
    let backend = Box::new(Git2Backend::open(fx.path()).unwrap());
    let app = App::new(Config::default(), backend, CompareSpec::Uncommitted)
        .expect("an embedded-repo directory must not crash App::new");
    assert!(
        !app.files()
            .iter()
            .any(|f| f.change.path.starts_with("vendored")),
        "the embedded-repo directory should be skipped, not listed as a file"
    );
}

#[test]
fn binary_file_diff_shows_indicator() {
    let fx = Fixture::new();
    fx.write_bytes("data.bin", &[0u8, 159, 146, 150, 1, 2, 3]);
    fx.commit("init");
    fx.write_bytes("data.bin", &[0u8, 1, 2, 3, 4, 5, 6, 7]);
    let app = app_from(&fx);
    let out = frame(&app, 80, 12);
    assert!(
        out.contains("No text diff"),
        "expected a binary indicator:\n{out}"
    );
}

#[test]
fn large_file_in_files_tab_renders_without_panic() {
    // Over HL_MAX_LINES → the per-visible-line highlight fallback path.
    let fx = Fixture::new();
    let big: String = (1..=3000).map(|n| format!("let v{n} = {n};\n")).collect();
    fx.write("big.rs", &big);
    fx.commit("init");
    fx.write("a.txt", "x\n"); // give the diff tab something too
    let mut app = app_from(&fx);
    app.handle_key(key('2')); // Files tab
    app.handle_key(key('j')); // onto big.rs (or a.txt) — load a file
                              // Render at a few sizes; the fallback path must not panic.
    for (w, h) in [(80, 24), (40, 10), (120, 50)] {
        let out = frame(&app, w, h);
        assert!(!out.is_empty());
    }
}

// ---- Per-gap expand (fold markers + Enter) ----

#[test]
fn diff_shows_fold_marker_and_enter_expands_it() {
    let fx = Fixture::new();
    // 40 unchanged lines with a single change near the middle → a big fold above
    // and below the change.
    let original: String = (1..=40).map(|n| format!("line{n}\n")).collect();
    fx.write("big.txt", &original);
    fx.commit("init");
    fx.write("big.txt", &original.replace("line20\n", "LINE20\n"));
    let mut app = app_from(&fx);

    // Before expanding: the diff has folds (fewer display rows than the 41 lines).
    let folded = app.display_rows().len();
    let fulllen = app.full_lines().len();
    assert!(
        folded < fulllen,
        "context should be folded ({folded} rows < {fulllen} lines)"
    );
    use gandr::diff::fold::DiffRow;
    let line_rows = |a: &App| {
        a.display_rows()
            .iter()
            .filter(|r| matches!(r, DiffRow::Line(_)))
            .count()
    };
    let has_fold = |a: &App| {
        a.display_rows()
            .iter()
            .any(|r| matches!(r, DiffRow::Fold { .. }))
    };
    assert!(has_fold(&app), "expected at least one fold marker");

    // Focus the diff and press Enter: a chunk of the active fold is revealed
    // (incremental — a big gap shrinks but stays a fold).
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
    let before = line_rows(&app);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
    let after = line_rows(&app);
    assert!(
        after > before,
        "Enter should reveal more lines ({before} → {after})"
    );
    assert!(
        after - before <= 10,
        "reveal is incremental (≤ EXPAND_STEP), got {}",
        after - before
    );
    // The rendered frame shows the marker (use a fresh, unexpanded app).
    let fresh = app_from(&fx);
    assert!(
        frame(&fresh, 80, 16).contains("unchanged lines"),
        "the fold marker should be visible in the diff"
    );
}

// ---- M11: expand context ----

#[test]
fn expand_context_reveals_more_lines() {
    let fx = Fixture::new();
    let base: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    fx.write("a.txt", &base);
    fx.commit("init");
    fx.write("a.txt", &base.replace("line 20\n", "line 20 CHANGED\n"));
    let mut app = app_from(&fx);

    let lines = |a: &App| -> usize {
        a.current()
            .map(|f| f.hunks.iter().map(|h| h.lines.len()).sum())
            .unwrap_or(0)
    };
    let before = lines(&app);

    app.handle_key(key('o')); // expand context 3 → 10
    app.refresh(); // synchronous recompute (the loop does this async)
    let after = lines(&app);

    assert!(
        after > before,
        "expanding context should reveal more lines: {before} → {after}"
    );
    assert!(
        app.header_line().contains("⊕10 ctx"),
        "header should advertise the expanded context: {}",
        app.header_line()
    );
}

// ---- M9: fuzzy ref picker ----

#[test]
fn ref_picker_lists_and_filters() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init"); // main
    fx.checkout_new_branch("feature/login");
    fx.checkout_new_branch("feature/logout");
    fx.checkout_new_branch("release-1.0");
    fx.tag("v1.0.0");
    fx.write("a.txt", "a changed\n");
    let mut app = app_from(&fx);

    app.handle_key(key('b')); // open the fuzzy ref picker
    for c in "log".chars() {
        // narrows to the two feature/log* branches, ranked
        app.handle_key(key(c));
    }
    insta::assert_snapshot!(frame(&app, 70, 14));
}

#[test]
fn ref_picker_enter_sets_comparison() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init"); // main
    fx.checkout_new_branch("feature/login");
    fx.write("a.txt", "a changed\n");
    let mut app = app_from(&fx);

    app.handle_key(key('b'));
    for c in "main".chars() {
        app.handle_key(key(c));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
    assert!(app.ref_picker().is_none(), "overlay closes on selection");
    // The comparison switched to "working tree vs main".
    assert!(
        app.header_line().contains("vs main"),
        "header should show the new comparison: {}",
        app.header_line()
    );
    // The rendered header shows the arrow to the compared-against ref.
    assert!(
        frame(&app, 90, 10).contains("→ main"),
        "header should point at the compared ref with an arrow"
    );
}

// ---- M5: compare picker ----

#[test]
fn compare_picker_overlay() {
    let fx = Fixture::new();
    fx.write("a.txt", "x\n");
    fx.commit("init");
    fx.write("a.txt", "y\n");
    let mut app = app_from(&fx);
    app.handle_key(key('c')); // open the compare picker
    insta::assert_snapshot!(frame(&app, 70, 14));
}

#[test]
fn m3_multibyte_and_long_lines_render_without_panic() {
    let fx = Fixture::new();
    fx.write("u.txt", "café ❤ 你好\nshort\n");
    fx.commit("init");
    let long = "x".repeat(500);
    fx.write("u.txt", format!("café ❤ 世界\n{long}\n").as_str());
    let app = app_from(&fx);
    // Narrow terminal exercises the fill/truncation and byte-slicing paths on
    // multibyte and over-wide lines; must not panic.
    let out = frame(&app, 24, 12);
    assert!(!out.is_empty());
}
