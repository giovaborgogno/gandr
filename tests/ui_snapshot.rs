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
