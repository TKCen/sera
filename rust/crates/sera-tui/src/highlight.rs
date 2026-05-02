//! Syntax highlighting for fenced code blocks — **J.1.2 (sera-jie3)**.
//!
//! Detects triple-backtick fences inside an assistant message body and
//! renders the body of each fence through `syntect` against the bundled
//! `base16-ocean.dark` theme.  Outside of fences, lines are emitted
//! verbatim as monospace `Span::raw`.  When the fence's language tag is
//! missing, unknown, or syntect lookup fails, the block falls back to a
//! plain monospace style so the content is never lost.
//!
//! ## Theme rationale
//!
//! `syntect`'s `default-themes` ship with five 16-colour palettes derived
//! from base16.  We pick `base16-ocean.dark` because:
//! * it is the canonical example used in syntect's own docs and tests,
//! * it is dark-friendly and high-contrast at 8-bit colour, matching the
//!   `Color::DarkGray`/`Color::Green` accents the rest of the TUI uses,
//! * it does not rely on bright/bold to differentiate token classes, which
//!   keeps the rendering legible on terminals that ignore those modifiers.
//!
//! Theme is hard-coded for this bead; a per-config override is a follow-up
//! for J.3.5 (theme customisation).
//!
//! ## Performance
//!
//! `SyntaxSet` and `ThemeSet` are loaded from the bundled binary dumps
//! once per process via `OnceCell`.  The first call to [`render_message`]
//! for a process pays a small one-time cost (~5–10 ms on a dev laptop)
//! deserialising the dumps; subsequent calls reuse the cached sets.

use once_cell::sync::Lazy;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// The default syntect syntax dump (Sublime Text grammars bundled with
/// the crate).  Loading is lazy because it deserialises ~2 MB of data.
static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);
/// Default theme set — bundled with syntect when the `default-themes`
/// feature is enabled.
static THEME_SET: Lazy<ThemeSet> = Lazy::new(ThemeSet::load_defaults);
/// Active theme.  See module docs for rationale.
static THEME: Lazy<&Theme> = Lazy::new(|| {
    THEME_SET
        .themes
        .get("base16-ocean.dark")
        .or_else(|| THEME_SET.themes.values().next())
        .expect("syntect default themes must include at least one entry")
});

/// Render an assistant message body into ratatui [`Line`]s, applying
/// syntect highlighting to triple-backtick fenced code blocks.
///
/// Lines outside of any fence are rendered verbatim with no style
/// (caller decides foreground/background).  Lines inside a fence are
/// styled per-token.  The fence delimiters themselves (` ``` ` and the
/// language tag) are emitted as dim borders so the operator can still
/// see where a block begins and ends.
pub fn render_message(text: &str) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut buf: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let mut fence_lang: Option<String> = None;

    for raw in text.split_inclusive('\n') {
        // Trim only the trailing newline; preserve internal whitespace
        // so syntect grammars that care about indentation parse correctly.
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix("```") {
            if in_fence {
                // Closing fence — flush buffered code lines through syntect.
                let code = buf.join("\n");
                let lines = highlight_code(&code, fence_lang.as_deref());
                out.extend(lines);
                out.push(fence_line(line));
                buf.clear();
                in_fence = false;
                fence_lang = None;
            } else {
                // Opening fence — record language tag (first whitespace-
                // delimited token after the backticks) and emit the fence
                // marker as a dim line so the operator sees the block
                // boundary.
                let lang = rest.split_whitespace().next().map(str::to_owned);
                fence_lang = lang.filter(|s| !s.is_empty());
                in_fence = true;
                out.push(fence_line(line));
            }
            continue;
        }
        if in_fence {
            buf.push(line);
        } else {
            out.push(Line::from(Span::raw(line.to_owned())));
        }
    }

    // Unterminated fence — render whatever was buffered as if the language
    // tag were valid.  The operator sees a partial code block instead of
    // losing the in-flight content.
    if in_fence && !buf.is_empty() {
        let code = buf.join("\n");
        out.extend(highlight_code(&code, fence_lang.as_deref()));
    }

    out
}

/// Render the fence delimiter line (e.g. ` ```rust `) in a dim style.
fn fence_line(raw: &str) -> Line<'static> {
    Line::from(Span::styled(
        raw.to_owned(),
        Style::default().fg(Color::DarkGray),
    ))
}

/// Highlight a multi-line `code` snippet.  Falls back to plain monospace
/// when `lang` is `None` or doesn't match any bundled syntax.
fn highlight_code(code: &str, lang: Option<&str>) -> Vec<Line<'static>> {
    let syntax = lang
        .and_then(|name| {
            SYNTAX_SET
                .find_syntax_by_token(name)
                .or_else(|| SYNTAX_SET.find_syntax_by_name(name))
        })
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, &THEME);
    let mut out: Vec<Line<'static>> = Vec::new();
    for line in LinesWithEndings::from(code) {
        // syntect can fail on malformed grammars or runaway regexes; a
        // failure here must not poison the whole transcript, so we fall
        // back to a plain span for that line and keep going.
        let regions = match highlighter.highlight_line(line, &SYNTAX_SET) {
            Ok(r) => r,
            Err(_) => {
                out.push(Line::from(Span::raw(strip_newline(line).to_owned())));
                continue;
            }
        };
        let spans: Vec<Span<'static>> = regions
            .into_iter()
            .filter_map(|(style, text)| {
                let trimmed = strip_newline(text);
                if trimmed.is_empty() {
                    None
                } else {
                    Some(Span::styled(trimmed.to_owned(), to_ratatui_style(style)))
                }
            })
            .collect();
        out.push(Line::from(spans));
    }
    out
}

fn strip_newline(s: &str) -> &str {
    s.strip_suffix('\n').unwrap_or(s)
}

/// Map a `syntect::highlighting::Style` (24-bit RGB) to a ratatui [`Style`].
/// Ratatui supports `Color::Rgb`, so the mapping is direct; we drop
/// background colours so the transcript keeps its terminal-default
/// background, and translate `FontStyle` flags to ratatui modifiers.
fn to_ratatui_style(style: SyntectStyle) -> Style {
    let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    let mut s = Style::default().fg(fg);
    if style.font_style.contains(FontStyle::BOLD) {
        s = s.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
    }

    #[test]
    fn plain_text_passes_through_unchanged() {
        let out = render_message("hello\nworld");
        assert_eq!(out.len(), 2);
        assert_eq!(line_text(&out[0]), "hello");
        assert_eq!(line_text(&out[1]), "world");
        // No styling on plain text — the only span is `Span::raw`.
        assert_eq!(out[0].spans[0].style, Style::default());
    }

    #[test]
    fn fenced_block_with_known_language_is_highlighted() {
        let msg = "before\n```rust\nfn main() {}\n```\nafter";
        let out = render_message(msg);
        // Lines: before, ```rust, fn main() {}, ```, after
        assert_eq!(out.len(), 5);
        assert_eq!(line_text(&out[0]), "before");
        assert_eq!(line_text(&out[1]), "```rust");
        assert_eq!(line_text(&out[2]), "fn main() {}");
        assert_eq!(line_text(&out[3]), "```");
        assert_eq!(line_text(&out[4]), "after");
        // Code line must be split into multiple styled spans (syntect
        // colours `fn` and `main` differently).
        assert!(
            out[2].spans.len() > 1,
            "expected syntect to split code into multiple spans, got {} spans",
            out[2].spans.len()
        );
        // At least one span carries an Rgb foreground from the theme.
        assert!(
            out[2]
                .spans
                .iter()
                .any(|sp| matches!(sp.style.fg, Some(Color::Rgb(..)))),
            "expected at least one RGB-styled span in highlighted code"
        );
    }

    #[test]
    fn fenced_block_without_language_falls_back_to_plain() {
        let msg = "```\njust text\n```";
        let out = render_message(msg);
        assert_eq!(out.len(), 3);
        assert_eq!(line_text(&out[1]), "just text");
        // Plaintext syntax has only one scope; the code line is one span.
        assert_eq!(out[1].spans.len(), 1);
    }

    #[test]
    fn fenced_block_with_unknown_language_falls_back_to_plain() {
        let msg = "```nosuchlang\nx = 1\n```";
        let out = render_message(msg);
        assert_eq!(out.len(), 3);
        assert_eq!(line_text(&out[1]), "x = 1");
    }

    #[test]
    fn unterminated_fence_still_renders_buffered_lines() {
        let msg = "```python\nprint('hi')\n";
        let out = render_message(msg);
        // Fence opener + the buffered line.
        assert_eq!(out.len(), 2);
        assert_eq!(line_text(&out[0]), "```python");
        assert_eq!(line_text(&out[1]), "print('hi')");
    }

    #[test]
    fn fence_delimiter_lines_use_dim_style() {
        let msg = "```rust\nfn main() {}\n```";
        let out = render_message(msg);
        assert_eq!(out[0].spans[0].style.fg, Some(Color::DarkGray));
        assert_eq!(out[2].spans[0].style.fg, Some(Color::DarkGray));
    }

    #[test]
    fn empty_input_produces_no_lines() {
        assert!(render_message("").is_empty());
    }

    #[test]
    fn multi_block_message_keeps_blocks_independent() {
        let msg = "intro\n```rust\nlet x = 1;\n```\nmid\n```python\ny = 2\n```\nend";
        let out = render_message(msg);
        // intro, ```rust, code, ```, mid, ```python, code, ```, end = 9 lines
        assert_eq!(out.len(), 9);
        assert_eq!(line_text(&out[0]), "intro");
        assert_eq!(line_text(&out[2]), "let x = 1;");
        assert_eq!(line_text(&out[4]), "mid");
        assert_eq!(line_text(&out[6]), "y = 2");
        assert_eq!(line_text(&out[8]), "end");
    }
}
