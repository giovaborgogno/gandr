//! Render a single gdiff frame to stdout as text, for headless inspection.
//!
//! `cargo run --example render` — the agent's "eyes" on the UI without a terminal.
//! Builds a small fixture repo, computes its diff, and renders one frame.

use anyhow::Result;
use gdiff::app::App;
use gdiff::config::Config;
use gdiff::git::git2_backend::Git2Backend;
use gdiff::git::CompareSpec;
use gdiff::testutil::Fixture;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn main() -> Result<()> {
    let fx = Fixture::new();
    fx.write("src/main.rs", "fn main() {\n    println!(\"hi\");\n}\n");
    fx.write("README.md", "# demo\n\nold line\n");
    fx.commit("init");
    fx.write(
        "src/main.rs",
        "fn main() {\n    let name = \"world\";\n    println!(\"hi {name}\");\n}\n",
    );
    fx.write("README.md", "# demo\n\nnew line\n");
    fx.write("notes.txt", "a brand new file\n");

    let backend = Box::new(Git2Backend::open(fx.path())?);
    let app = App::new(Config::default(), backend, CompareSpec::Uncommitted)?;

    let mut terminal = Terminal::new(TestBackend::new(100, 24))?;
    terminal.draw(|f| app.render(f))?;
    println!("{}", terminal.backend());
    Ok(())
}
