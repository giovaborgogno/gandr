//! Snapshot tests: drive the real `App`, render to `TestBackend`, golden the frame.
//! This is gdiff's deterministic headless "e2e" (see docs/testing.md).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gdiff::app::App;
use gdiff::config::Config;
use gdiff::diff::engine;
use gdiff::git::git2_backend::Git2Backend;
use gdiff::git::{CompareSpec, GitBackend};
use gdiff::testutil::Fixture;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;

/// Build an `App` showing the uncommitted diff of a fixture.
fn app_from(fx: &Fixture) -> App {
    let backend = Git2Backend::open(fx.path()).unwrap();
    let context = backend.context().unwrap();
    let files = engine::build_diffs(&backend, &CompareSpec::Uncommitted, 3).unwrap();
    App::new(Config::default(), context, CompareSpec::Uncommitted, files)
}

/// Render the App to a text frame of the given size.
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
    let app = app_from(&fx);

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
