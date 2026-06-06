//! Render a gdiff frame to an HTML file with real cell colors, so the UI can be
//! screenshotted faithfully (the TUI is otherwise only inspectable as text).
//!
//! `cargo run --example html_render -- <out.html>` (defaults to gdiff.html).

use anyhow::Result;
use gdiff::app::App;
use gdiff::config::Config;
use gdiff::git::git2_backend::Git2Backend;
use gdiff::git::CompareSpec;
use gdiff::highlight::ThemeMode;
use gdiff::testutil::Fixture;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

fn color_css(c: Color) -> Option<String> {
    match c {
        Color::Reset => None,
        Color::Rgb(r, g, b) => Some(format!("rgb({r},{g},{b})")),
        Color::Green => Some("#3fb950".into()),
        Color::Red => Some("#f85149".into()),
        Color::Cyan => Some("#39c5cf".into()),
        Color::Blue => Some("#58a6ff".into()),
        Color::Yellow => Some("#d29922".into()),
        Color::DarkGray => Some("#6e7681".into()),
        Color::Gray => Some("#b1bac4".into()),
        _ => None,
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn main() -> Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gdiff.html".into());

    // A representative scenario: a modified Rust file in a dir (tree compaction),
    // a README change (word-level), and a new file.
    let fx = Fixture::new();
    fx.write(
        "src/app/main.rs",
        "fn main() {\n    let greeting = \"hello\";\n    println!(\"{greeting}\");\n}\n",
    );
    fx.write("README.md", "# gdiff\n\nA diff viewer for the terminal.\n");
    fx.commit("init");
    fx.write(
        "src/app/main.rs",
        "fn main() {\n    let greeting = \"hello, world\";\n    for _ in 0..3 {\n        println!(\"{greeting}\");\n    }\n}\n",
    );
    fx.write(
        "README.md",
        "# gdiff\n\nA delta-style diff viewer for the terminal.\n",
    );
    fx.write("notes.txt", "todo: ship it\n");

    let backend = Box::new(Git2Backend::open(fx.path())?);
    let mut app = App::new(Config::default(), backend, CompareSpec::Uncommitted)?;
    app.set_theme_mode(ThemeMode::Dark);

    // Scenario keywords (any combination) drive the App into a feature's state.
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let key = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty());
    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::empty());
    let args: Vec<String> = std::env::args().collect();
    let has = |s: &str| args.iter().any(|a| a == s);

    if has("split") {
        app.handle_key(key('s'));
    }
    if has("wordoff") {
        app.handle_key(key('w'));
    }
    if has("review") {
        app.handle_key(key(' ')); // review the first file (main.rs)
        app.handle_key(key('n')); // → next file (README.md)
        app.handle_key(key(' ')); // review it too
                                  // Now change README on disk and refresh → it becomes "changed since reviewed".
        fx.write("README.md", "# gdiff\n\nA fast delta-style diff viewer.\n");
        app.refresh();
    }
    if has("search") {
        app.handle_key(key('/'));
        for c in "greeting".chars() {
            app.handle_key(key(c));
        }
        app.handle_key(enter);
    }
    if has("help") {
        app.handle_key(key('?'));
    }
    if has("picker") {
        app.handle_key(key('c'));
    }

    let env_u16 = |k: &str, d: u16| {
        std::env::var(k)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(d)
    };
    let (w, h) = (env_u16("GDIFF_COLS", 110), env_u16("GDIFF_ROWS", 30));
    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    let completed = terminal.draw(|f| app.render(f))?;
    let buf = completed.buffer;

    let mut body = String::new();
    for y in 0..h {
        for x in 0..w {
            let cell = buf.cell((x, y)).expect("cell");
            let reversed = cell.modifier.contains(Modifier::REVERSED);
            let (mut fg, mut bg) = (color_css(cell.fg), color_css(cell.bg));
            if reversed {
                std::mem::swap(&mut fg, &mut bg);
                fg = fg.or_else(|| Some("#0d1117".into()));
                bg = bg.or_else(|| Some("#b1bac4".into()));
            }
            let mut style = String::new();
            if let Some(fg) = fg {
                style.push_str(&format!("color:{fg};"));
            }
            if let Some(bg) = bg {
                style.push_str(&format!("background:{bg};"));
            }
            if cell.modifier.contains(Modifier::BOLD) {
                style.push_str("font-weight:bold;");
            }
            if cell.modifier.contains(Modifier::DIM) {
                style.push_str("opacity:0.6;");
            }
            body.push_str(&format!(
                "<span style=\"{style}\">{}</span>",
                esc(cell.symbol())
            ));
        }
        body.push('\n');
    }

    let html = format!(
        "<!doctype html><meta charset=utf-8><body style=\"margin:0;background:#0d1117\">\
         <pre style=\"font:15px/1.25 'JetBrains Mono','SF Mono',Menlo,monospace;\
         color:#c9d1d9;background:#0d1117;padding:16px;display:inline-block\">{body}</pre>"
    );
    std::fs::write(&out, html)?;
    println!("wrote {out}");
    Ok(())
}
