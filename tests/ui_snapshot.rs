//! Snapshot tests: drive the real `App`, render to `TestBackend`, golden the frame.
//! This is gdiff's deterministic headless "e2e" (see docs/testing.md).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gdiff::app::App;
use gdiff::config::Config;
use gdiff::git::git2_backend::Git2Backend;
use gdiff::git::CompareSpec;
use gdiff::testutil::Fixture;
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
    use gdiff::review::ReviewStatus;
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
    use gdiff::highlight::ThemeMode;
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
        let results = gdiff::search::run(root, &query, mode);
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
    app.handle_key(key('/')); // open repo search (content mode)
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
    app.handle_key(key('/')); // open repo search
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty())); // → file-name mode
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
    app.handle_key(key('/'));
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
    assert_eq!(app.browser().content_scroll(), 2);
}

// ---- M12 (diff viewer): multi-line syntax highlighting ----

/// Drive the async highlight job to completion (no run_loop in tests), the same
/// flow the event loop performs on selection.
fn warm_highlight(app: &mut App) {
    if let Some((epoch, path, old, new, mode)) = app.take_pending_highlight() {
        let hl = gdiff::highlight::Highlighter::for_path(&path, mode);
        let o = hl.highlight_file(&gdiff::diff::engine::split_lines(&old));
        let n = hl.highlight_file(&gdiff::diff::engine::split_lines(&new));
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

// ---- edge cases: render without panic ----

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
    use gdiff::diff::fold::DiffRow;
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
