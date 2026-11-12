//! Rendered Markdown preview (M16, see ADR 0009).
//!
//! `m` flips a Markdown file between its source (the normal diff / file view)
//! and a rendered, "GitHub-readme-like" preview. Parsing + layout is delegated
//! to `tui-markdown` (pulldown-cmark → ratatui `Text`); this module adapts its
//! output to gandr: a theme-aware style sheet, word wrapping to the pane width
//! (tables are sized by `tui-markdown` itself), nicer bullets / task boxes /
//! rules, heading underlines, and fenced code blocks highlighted with our own
//! syntect setup (so we don't pull `tui-markdown`'s oniguruma-based highlighter).
//! ```mermaid blocks are drawn as Unicode box diagrams by `mermaid-text`
//! (falling back to the highlighted source when a diagram can't be parsed).
//!
//! Everything here is render-pure: `source text + width + theme → Vec<Line>`.
//! The app caches the result per (file, content, width, theme).

use crate::highlight::{Highlighter, ThemeMode};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;
use tui_markdown::{Options, StyleSheet};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Code-fence delimiter we ask `tui-markdown` to emit: a private-use char, so a
/// code line that *contains* "```" (e.g. a snippet inside a ```` block) is never
/// mistaken for a fence.
const FENCE: &str = "\u{E000}";

/// Largest Markdown source rendered as a preview. Layout + code highlighting run
/// on the render thread (on a cache miss: opening the file or resizing), measured
/// at ~80ms for 160 KB in the dev profile; bigger files show a notice instead of
/// freezing the UI. (Async rendering is a backlog follow-up.)
pub const MAX_PREVIEW_BYTES: usize = 256 * 1024;

/// Largest ```mermaid block (non-blank source lines) drawn as a diagram. Layout
/// is superlinear (~0.2s at 100 nodes, ~1.5s at 200 in the dev profile) and runs
/// on the UI thread on a cache miss; bigger diagrams show their source.
const MAX_DIAGRAM_LINES: usize = 100;

/// Narrowest code box (columns) a diagram is drawn in; below it, the source.
const MIN_DIAGRAM_WIDTH: usize = 8;

/// Spaces a tab expands to inside the preview (ratatui can't measure `\t`).
const TAB: &str = "    ";

/// Whether `path` looks like a Markdown file (by extension, case-insensitive).
pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| {
            matches!(
                e.as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdx"
            )
        })
}

/// gandr's Markdown style sheet: readable on both light and dark terminals.
#[derive(Debug, Clone, Copy)]
struct Styles {
    mode: ThemeMode,
}

impl Styles {
    fn accent(&self) -> Color {
        match self.mode {
            ThemeMode::Dark => Color::Cyan,
            ThemeMode::Light => Color::Blue,
        }
    }

    fn code_bg(&self) -> Color {
        match self.mode {
            ThemeMode::Dark => Color::Rgb(40, 44, 52),
            ThemeMode::Light => Color::Rgb(238, 240, 243),
        }
    }

    fn code_fg(&self) -> Color {
        match self.mode {
            ThemeMode::Dark => Color::Rgb(220, 223, 228),
            ThemeMode::Light => Color::Rgb(36, 41, 47),
        }
    }
}

impl StyleSheet for Styles {
    fn heading(&self, level: u8) -> Style {
        let base = Style::new().fg(self.accent()).add_modifier(Modifier::BOLD);
        match level {
            // Each level gets a distinct style: `render` picks the H1/H2 underline
            // rule by comparing line styles, so H3 must not equal H2.
            1 => base.add_modifier(Modifier::UNDERLINED),
            2 => base,
            3 => Style::new().add_modifier(Modifier::BOLD),
            _ => Style::new().add_modifier(Modifier::BOLD | Modifier::ITALIC),
        }
    }

    fn heading_marker(&self, _level: u8) -> &str {
        "" // headings are shown by style (and an underline rule for H1/H2), not `#`
    }

    fn code_block_fence(&self) -> &str {
        FENCE // a sentinel `render` strips; real code lines can't collide with it
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::DarkGray) // YAML front matter, readable on both themes
    }

    fn code(&self) -> Style {
        Style::new().fg(self.code_fg()).bg(self.code_bg())
    }

    fn link(&self) -> Style {
        Style::new().fg(Color::Blue).underlined()
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::DarkGray).italic()
    }

    fn alert_icon(&self, _kind: tui_markdown::AlertKind) -> &str {
        // The default icons are emoji with variation selectors whose terminal
        // width is ambiguous (they corrupt the row's right edge); the colored
        // bold label alone is clear.
        ""
    }

    fn list_marker(&self) -> Style {
        Style::new().fg(self.accent())
    }
}

/// Render `src` as Markdown into display lines `width` columns wide.
pub fn render(src: &str, width: u16, mode: ThemeMode) -> Vec<Line<'static>> {
    let width = width.max(1);
    let styles = Styles { mode };
    let options = Options::new(styles).table_width(width);
    let text = tui_markdown::from_str_with_options(src, &options);
    let code_style = styles.code();
    let w = width as usize;

    let mut out: Vec<Line<'static>> = Vec::new();
    // The open fenced code block, buffered so it's highlighted as a whole.
    let mut code: Option<CodeBlock> = None;
    for line in text.lines {
        let line = owned(line);
        let is_code = line.style == code_style;

        if is_code {
            // Fence delimiter: spans before the sentinel are the enclosing
            // prefix (blockquote `> `, …); the text after it is the info string.
            if let Some(i) = line.spans.iter().position(|s| s.content.starts_with(FENCE)) {
                match code.take() {
                    Some(block) => out.extend(block.finish(styles, w)),
                    None => {
                        let info = line.spans[i].content.trim_start_matches(FENCE).to_string();
                        let prefix = line.spans[..i].to_vec();
                        code = Some(CodeBlock::new(&info, prefix, mode));
                    }
                }
                continue;
            }
            if let Some(block) = code.as_mut() {
                block.push(&line);
                continue;
            }
        }

        // Thematic break → a full-width rule.
        if line.style == Style::default() && plain_text(&line) == "---" {
            out.push(Line::from(Span::styled(
                "─".repeat(w),
                Style::new().dark_gray(),
            )));
            continue;
        }

        let heading_rule = [1u8, 2]
            .into_iter()
            .find(|l| line.style == styles.heading(*l));
        let line = prettify_marker(line);
        out.extend(wrap(line, w));
        if let Some(level) = heading_rule {
            let ch = if level == 1 { "━" } else { "─" };
            out.push(Line::from(Span::styled(
                ch.repeat(w),
                Style::new().dark_gray(),
            )));
        }
    }
    if let Some(block) = code {
        out.extend(block.finish(styles, w)); // unterminated block: flush what we have
    }
    out
}

/// A fenced code block being collected: its enclosing prefix spans (repeated on
/// every row), its language highlighter, and its lines.
struct CodeBlock {
    prefix: Vec<Span<'static>>,
    prefix_text: String,
    hl: Option<Highlighter>,
    /// A ```mermaid block: drawn as a diagram instead of highlighted source.
    mermaid: bool,
    lines: Vec<String>,
}

impl CodeBlock {
    fn new(info: &str, prefix: Vec<Span<'static>>, mode: ThemeMode) -> Self {
        let prefix_text = prefix.iter().map(|s| s.content.as_ref()).collect();
        Self {
            prefix,
            prefix_text,
            hl: code_highlighter(info, mode),
            mermaid: info
                .split_whitespace()
                .next()
                .is_some_and(|l| l.eq_ignore_ascii_case("mermaid")),
            lines: Vec::new(),
        }
    }

    /// Add a code line, dropping the enclosing prefix `tui-markdown` repeats.
    fn push(&mut self, line: &Line<'static>) {
        let plain = plain_text(line);
        let body = plain
            .strip_prefix(self.prefix_text.as_str())
            .unwrap_or(&plain);
        self.lines.push(body.to_string());
    }

    /// Highlight the block with carried syntect state (multi-line comments and
    /// strings color correctly) and lay it out as a padded box.
    fn finish(self, styles: Styles, w: usize) -> Vec<Line<'static>> {
        let base = styles.code();
        let pw = self.prefix_text.width();
        let inner = w.saturating_sub(pw).max(1);
        let mut out = Vec::new();
        if self.mermaid {
            match self.diagram(styles, inner) {
                Ok(rows) => return rows,
                // Shown above the source it falls back to.
                Err(Some(note)) => out.push(self.code_row(note, base.italic(), base, inner)),
                Err(None) => {}
            }
        }
        let highlights = match &self.hl {
            Some(hl) => hl.highlight_file(&self.lines),
            None => Vec::new(),
        };
        for (i, text) in self.lines.iter().enumerate() {
            let mut spans = vec![Span::styled(" ", base)];
            let fg = highlights.get(i).filter(|f| !f.is_empty());
            match fg {
                Some(fg) => spans.extend(fg.iter().filter_map(|s| {
                    let piece = text.get(s.start..s.end)?;
                    Some(Span::styled(piece.to_string(), base.fg(s.color)))
                })),
                None => spans.push(Span::styled(text.clone(), base)),
            }
            for mut row in hard_wrap(spans, inner) {
                let used: usize = row.iter().map(|s| s.content.width()).sum();
                if used < inner {
                    row.push(Span::styled(" ".repeat(inner - used), base));
                }
                let mut full = self.prefix.clone();
                full.extend(row);
                out.push(Line::from(full));
            }
        }
        out
    }

    /// Draw a ```mermaid block as a box diagram fitted to `inner` columns.
    /// `Err` means "show the source instead", with an optional note: the diagram
    /// is too large to lay out on the UI thread, or `mermaid-text` can't parse
    /// it (or panicked on it). Rows are clipped (marked `…`), never wrapped:
    /// wrapping would scramble the drawing, and the renderer already tries to
    /// fit the width it's given.
    fn diagram(&self, styles: Styles, inner: usize) -> Result<Vec<Line<'static>>, Option<String>> {
        let base = styles.code();
        if inner < MIN_DIAGRAM_WIDTH {
            return Err(None);
        }
        let statements = self.lines.iter().filter(|l| !l.trim().is_empty()).count();
        if statements > MAX_DIAGRAM_LINES {
            return Err(Some(format!(
                "Mermaid diagram too large to draw ({statements} lines > {MAX_DIAGRAM_LINES}); showing its source."
            )));
        }
        let budget = inner - 1; // 1 col of left padding
        let src = self.lines.join("\n");
        let drawn =
            crate::panic_guard::contain(|| mermaid_text::render_with_width(&src, Some(budget)))
                .and_then(Result::ok)
                .ok_or(None)?;
        let mut out = vec![self.code_row(String::new(), base, base, inner)];
        for text in drawn.lines() {
            let row = if text.width() > budget {
                let mut cut = clip(text, budget - 1);
                cut.push('…');
                cut
            } else {
                text.to_string()
            };
            out.push(self.code_row(row, base, base, inner));
        }
        out.push(self.code_row(String::new(), base, base, inner));
        Ok(out)
    }

    /// One row of the code box: prefix, 1 col of padding, `text` (clipped) in
    /// `style`, then `base` padding to `inner` columns.
    fn code_row(&self, text: String, style: Style, base: Style, inner: usize) -> Line<'static> {
        let budget = inner.saturating_sub(1);
        let text = clip(&text, budget);
        let pad = budget - text.width();
        let mut row = self.prefix.clone();
        row.push(Span::styled(" ", base));
        row.push(Span::styled(text, style));
        if pad > 0 {
            row.push(Span::styled(" ".repeat(pad), base));
        }
        Line::from(row)
    }
}

/// The longest prefix of `s` that fits in `max` display columns.
fn clip(s: &str, max: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw > max {
            break;
        }
        used += cw;
        out.push(c);
    }
    out
}

/// Detach a borrowed `tui-markdown` line from the source text (and expand tabs).
fn owned(line: Line<'_>) -> Line<'static> {
    let spans: Vec<Span<'static>> = line
        .spans
        .into_iter()
        .map(|s| Span::styled(s.content.replace('\t', TAB), s.style))
        .collect();
    Line {
        spans,
        style: line.style,
        alignment: line.alignment,
    }
}

fn plain_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// A syntect highlighter for a fence info string's language token, if known.
fn code_highlighter(info: &str, mode: ThemeMode) -> Option<Highlighter> {
    let lang = info.split_whitespace().next()?;
    Highlighter::for_token(lang, mode)
}

/// Swap ASCII list markers for typographic ones: `-` → `•`/`◦`/`▪` by depth,
/// task items `- [ ]`/`- [x]` → `☐`/`☑`.
fn prettify_marker(mut line: Line<'static>) -> Line<'static> {
    let Some(first) = line.spans.first_mut() else {
        return line;
    };
    let s = first.content.as_ref();
    let trimmed = s.trim_start();
    let indent = &s[..s.len() - trimmed.len()];
    let bullet = match indent.len() / 4 {
        0 => "•",
        1 => "◦",
        _ => "▪",
    };
    let replaced = match trimmed {
        "- " | "* " | "+ " => Some(format!("{indent}{bullet} ")),
        "- [ ] " | "* [ ] " | "+ [ ] " => Some(format!("{indent}☐ ")),
        "- [x] " | "- [X] " | "* [x] " | "* [X] " | "+ [x] " | "+ [X] " => {
            Some(format!("{indent}☑ "))
        }
        _ => None,
    };
    if let Some(r) = replaced {
        first.content = r.into();
    }
    line
}

/// The hanging-indent prefix for a wrapped line's continuation rows: the
/// leading list marker / blockquote bars become spaces (bars are kept), so
/// wrapped text lines up under the first row's text.
fn continuation_prefix(line: &Line<'static>) -> Option<(String, Style)> {
    let first = line.spans.first()?;
    let s = first.content.as_ref();
    let is_marker = {
        let t = s.trim();
        !t.is_empty()
            && s.ends_with(' ')
            && (matches!(t, "•" | "◦" | "▪" | "☐" | "☑" | "-" | "*" | "+")
                || t.trim_end_matches(['.', ')'])
                    .chars()
                    .all(|c| c.is_ascii_digit()))
    };
    if is_marker {
        return Some((" ".repeat(s.width()), Style::default()));
    }
    // Blockquote: leading `>` spans (and their separating spaces) repeat.
    let mut prefix = String::new();
    for span in &line.spans {
        let c = span.content.as_ref();
        if c.chars().all(|ch| ch == '>' || ch == ' ') && !c.is_empty() {
            prefix.push_str(c);
        } else {
            break;
        }
    }
    if prefix.contains('>') {
        return Some((prefix, line.style));
    }
    // Plain indentation (e.g. a list item's continuation paragraph).
    let lead = s.len() - s.trim_start().len();
    (lead > 0).then(|| (" ".repeat(lead), Style::default()))
}

/// Word-wrap a styled line to `w` columns, keeping span styles and the line's
/// own style, with a hanging indent for list items / blockquotes.
fn wrap(line: Line<'static>, w: usize) -> Vec<Line<'static>> {
    if line.width() <= w {
        return vec![line];
    }
    let (prefix, prefix_style) = continuation_prefix(&line)
        .filter(|(p, _)| p.width() < w / 2)
        .unwrap_or_default();
    let prefix_w = prefix.width();
    let style = line.style;

    // Tokenize into (text, style, is_space) runs, splitting every span on spaces.
    let mut tokens: Vec<(String, Style, bool)> = Vec::new();
    for span in &line.spans {
        let mut cur = String::new();
        let mut cur_space = None;
        for ch in span.content.chars() {
            let sp = ch == ' ';
            if cur_space.is_some_and(|s| s != sp) {
                tokens.push((
                    std::mem::take(&mut cur),
                    span.style,
                    cur_space == Some(true),
                ));
            }
            cur.push(ch);
            cur_space = Some(sp);
        }
        if !cur.is_empty() {
            tokens.push((cur, span.style, cur_space == Some(true)));
        }
    }

    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut used = 0usize;
    // Whether the current row has any content past its prefix yet.
    let mut has_content = false;
    // A space run between words, emitted only if the next word fits on the same
    // row (so wrapped rows never end — or start — with a stray space).
    let mut pending: Option<(String, Style)> = None;
    let new_row = |rows: &mut Vec<Vec<Span<'static>>>, used: &mut usize| {
        let mut row = Vec::new();
        if prefix_w > 0 {
            row.push(Span::styled(prefix.clone(), prefix_style));
        }
        rows.push(row);
        *used = prefix_w;
    };
    for (i, (text, tstyle, is_space)) in tokens.into_iter().enumerate() {
        let tw = text.width();
        if is_space {
            if i == 0 {
                // Leading indentation is structure, not a word gap: keep it.
                push_span(&mut rows, text, tstyle);
                used += tw;
            } else {
                pending = Some((text, tstyle));
            }
            continue;
        }
        let pw = pending.as_ref().map_or(0, |(p, _)| p.width());
        if has_content && used + pw + tw > w {
            new_row(&mut rows, &mut used);
            pending = None;
        } else if let Some((p, ps)) = pending.take() {
            push_span(&mut rows, p, ps);
            used += pw;
        }
        if used + tw <= w {
            push_span(&mut rows, text, tstyle);
            used += tw;
        } else {
            // A single word longer than the row: hard-split it.
            for ch in text.chars() {
                let cw = ch.width().unwrap_or(0);
                if used + cw > w && used > prefix_w {
                    new_row(&mut rows, &mut used);
                }
                push_span(&mut rows, ch.to_string(), tstyle);
                used += cw;
            }
        }
        has_content = true;
    }
    rows.into_iter()
        .map(|spans| Line::from(spans).style(style))
        .collect()
}

/// Append text to the last row, merging into the previous span when styles match.
fn push_span(rows: &mut [Vec<Span<'static>>], text: String, style: Style) {
    let Some(row) = rows.last_mut() else {
        return;
    };
    if let Some(last) = row.last_mut() {
        if last.style == style {
            last.content.to_mut().push_str(&text);
            return;
        }
    }
    row.push(Span::styled(text, style));
}

/// Hard-wrap spans (by display width) into rows of at most `w` columns.
fn hard_wrap(spans: Vec<Span<'static>>, w: usize) -> Vec<Vec<Span<'static>>> {
    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut used = 0;
    for span in spans {
        for ch in span.content.chars() {
            let cw = ch.width().unwrap_or(0);
            if used + cw > w && used > 0 {
                rows.push(Vec::new());
                used = 0;
            }
            push_span(&mut rows, ch.to_string(), span.style);
            used += cw;
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(plain_text).collect()
    }

    #[test]
    fn detects_markdown_extensions() {
        assert!(is_markdown_path(Path::new("README.md")));
        assert!(is_markdown_path(Path::new("docs/x.MARKDOWN")));
        assert!(is_markdown_path(Path::new("a.mdx")));
        assert!(!is_markdown_path(Path::new("main.rs")));
        assert!(!is_markdown_path(Path::new("md")));
    }

    #[test]
    fn renders_headings_lists_and_rules() {
        let out = text_of(&render(
            "# Title\n\n- one\n  - two\n- [ ] todo\n- [x] done\n\n---\n",
            20,
            ThemeMode::Dark,
        ));
        assert_eq!(out[0], "Title");
        assert_eq!(out[1], "━".repeat(20)); // H1 underline
        assert!(out.contains(&"• one".to_string()), "{out:?}");
        assert!(out.contains(&"    ◦ two".to_string()), "{out:?}");
        assert!(out.contains(&"☐ todo".to_string()), "{out:?}");
        assert!(out.contains(&"☑ done".to_string()), "{out:?}");
        assert!(out.contains(&"─".repeat(20)), "{out:?}");
    }

    #[test]
    fn wraps_prose_with_hanging_indent() {
        let out = text_of(&render(
            "- alpha beta gamma delta epsilon\n",
            14,
            ThemeMode::Dark,
        ));
        assert_eq!(out, vec!["• alpha beta", "  gamma delta", "  epsilon"]);
        assert!(out.iter().all(|l| l.width() <= 14));
    }

    #[test]
    fn code_block_drops_fences_and_pads() {
        let lines = render("```rust\nlet x = 1;\n```\n", 16, ThemeMode::Dark);
        let out = text_of(&lines);
        assert_eq!(out, vec![" let x = 1;     "]);
        // Syntax colored: more than one distinct foreground on the code line.
        let fgs: std::collections::HashSet<_> =
            lines[0].spans.iter().filter_map(|s| s.style.fg).collect();
        assert!(fgs.len() > 1, "expected syntax colors, got {fgs:?}");
    }

    #[test]
    fn code_fences_inside_code_and_quotes() {
        // A ``` line inside a ```` block is code, not a fence.
        let out = text_of(&render(
            "````md\n```rust\nx\n```\n````\n\nafter\n",
            12,
            ThemeMode::Dark,
        ));
        assert_eq!(
            out,
            vec![" ```rust    ", " x          ", " ```        ", "", "after"]
        );
        // A fenced block inside a blockquote keeps the `> ` prefix, no raw fences.
        let out = text_of(&render("> ```sh\n> echo hi\n> ```\n", 14, ThemeMode::Dark));
        assert_eq!(out, vec![">  echo hi    "]);
    }

    #[test]
    fn code_block_highlight_carries_state() {
        let lines = render(
            "```rust\n/* open\nstill comment\n*/\n```\n",
            30,
            ThemeMode::Dark,
        );
        let fg = |i: usize| {
            lines[i]
                .spans
                .iter()
                .find(|s| !s.content.trim().is_empty())
                .and_then(|s| s.style.fg)
        };
        assert_eq!(
            fg(0),
            fg(1),
            "interior comment line should share the comment color"
        );
    }

    #[test]
    fn h3_has_no_underline_rule() {
        let out = text_of(&render("### Install\n", 10, ThemeMode::Dark));
        assert_eq!(out, vec!["Install"]);
    }

    #[test]
    fn long_words_and_tables_fit_width() {
        let src = "averyveryverylongwordthatcannotfit\n\n| a | b |\n|---|---|\n| one two three four | x |\n";
        let lines = render(src, 12, ThemeMode::Light);
        for l in &lines {
            assert!(l.width() <= 12, "too wide: {:?}", plain_text(l));
        }
    }

    #[test]
    fn mermaid_block_draws_a_diagram() {
        let src = "```mermaid\ngraph LR; A[Build] --> B[Test]\n```\n";
        let lines = render(src, 40, ThemeMode::Dark);
        let out = text_of(&lines);
        let all = out.join("\n");
        assert!(all.contains("│ Build │"), "{all}");
        assert!(all.contains("│ Test │"), "{all}");
        assert!(!all.contains("-->"), "source leaked: {all}");
        // A padded box: every row exactly the pane width.
        assert!(out.iter().all(|l| l.width() == 40), "{out:?}");
    }

    #[test]
    fn mermaid_fits_narrow_panes_and_falls_back_on_bad_input() {
        let src = "```mermaid\ngraph LR; A[A very long build label] --> B[An equally long test label] --> C[Deploy]\n```\n";
        for w in [12u16, 30, 50] {
            let out = text_of(&render(src, w, ThemeMode::Dark));
            assert!(
                out.iter().all(|l| l.width() == w as usize),
                "w={w}: {out:?}"
            );
        }
        // Not parseable as a diagram → the source is shown, highlighted as code.
        let out = text_of(&render(
            "```mermaid\nnot a diagram ][\n```\n",
            24,
            ThemeMode::Dark,
        ));
        assert_eq!(out, vec![" not a diagram ][       "]);
    }

    #[test]
    fn oversized_mermaid_shows_its_source_with_a_note() {
        let body: String = (0..=MAX_DIAGRAM_LINES)
            .map(|i| format!("  N{i} --> N{}\n", i + 1))
            .collect();
        let src = format!("```mermaid\ngraph TD\n{body}```\n");
        let out = text_of(&render(&src, 60, ThemeMode::Dark));
        assert!(out[0].contains("too large to draw"), "{:?}", &out[..2]);
        assert!(out[1].contains("graph TD"), "{:?}", &out[..2]);
        assert!(out.iter().all(|l| l.width() == 60));
    }

    #[test]
    fn clipped_diagram_rows_are_marked() {
        // Far too wide to compact into 10 columns: rows get clipped with `…`.
        let src = "```mermaid\ngraph LR; A[Alpha] --> B[Bravo] --> C[Charlie]\n```\n";
        let out = text_of(&render(src, 10, ThemeMode::Dark));
        assert!(out.iter().any(|l| l.contains('…')), "{out:?}");
        assert!(out.iter().all(|l| l.width() == 10), "{out:?}");
    }
}
