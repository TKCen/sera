//! Progressive markdown renderer — **J.1.1 (sera-7fpx)**.
//!
//! Converts a markdown `&str` to a `Vec<Line<'static>>` suitable for
//! ratatui's `List` / `Paragraph` widgets.  Called by
//! [`super::session::block_to_list_items`] when rendering a
//! [`super::blocks::Block::AssistantMessage`].
//!
//! ## Supported syntax
//!
//! * Paragraphs — plain text, wrapped at newlines.
//! * **Bold** (`**` / `__`), *italic* (`*` / `_`), `inline code` (`` ` ``).
//! * `# H1` … `###### H6` — rendered bold + coloured, prefix stripped.
//! * Unordered lists (`- ` / `* `) — `• ` bullet prefix.
//! * Ordered lists — `1. ` numeric prefix.
//! * Links — `[text](url)` → `text (url)` in dim style.
//! * Code blocks (fenced or indented) — rendered as monospace cyan text.
//!   Syntax highlighting is J.1.2 (sera-jie3).
//!
//! ## Streaming / flicker avoidance
//!
//! [`md_to_lines`] renders whatever text is passed to it.  The **caller**
//! is responsible for boundary-buffering: when an `AssistantMessage` block
//! is still `streaming=true`, only the text up to the last blank-line
//! boundary should be passed here; the remainder should be rendered as a
//! plain unstyled trailing line.  [`split_at_boundary`] is the helper for
//! that pattern.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Convert markdown source to ratatui [`Line`]s.
///
/// Every logical rendered line becomes one [`Line`].  Blank separator
/// lines between blocks are included so the visual layout matches
/// expectations.
pub fn md_to_lines(markdown: &str) -> Vec<Line<'static>> {
    let mut renderer = Renderer::default();
    let parser = Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH);
    renderer.process(parser);
    renderer.finish()
}

/// Split `text` at the last "block boundary" (blank line) for streaming.
///
/// Returns `(committed, in_flight)`:
/// * `committed` — text up to and including the last blank-line boundary;
///   safe to pass to [`md_to_lines`] without flicker.
/// * `in_flight` — the remainder that has not yet formed a complete block;
///   render as a plain unstyled line.
///
/// If there is no blank-line boundary, `committed` is `""` and
/// `in_flight` is the whole string.
pub fn split_at_boundary(text: &str) -> (&str, &str) {
    if let Some(pos) = last_blank_line_end(text) {
        let committed = &text[..pos];
        let in_flight = text[pos..].trim_start_matches('\n');
        (committed, in_flight)
    } else {
        ("", text)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Return the byte offset just after the last `\n\n` sequence in `s`,
/// consuming any additional consecutive newlines.
fn last_blank_line_end(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut last = None;
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'\n' && b[i + 1] == b'\n' {
            let mut end = i + 2;
            while end < b.len() && b[end] == b'\n' {
                end += 1;
            }
            last = Some(end);
            i = end;
        } else {
            i += 1;
        }
    }
    last
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

/// Inline-style accumulator — tracks bold / italic / code / link / strike.
#[derive(Default, Clone)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    code: bool,
    strikethrough: bool,
    link_url: Option<String>,
}

impl InlineStyle {
    fn to_ratatui(&self) -> Style {
        let mut s = Style::default();
        if self.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            s = s.add_modifier(Modifier::ITALIC);
        }
        if self.code {
            s = s.fg(Color::Cyan);
        }
        if self.strikethrough {
            s = s.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.link_url.is_some() {
            s = s.fg(Color::Blue).add_modifier(Modifier::UNDERLINED);
        }
        s
    }
}

/// Tag-stack context so `on_end` knows how to close each block.
#[derive(Clone)]
enum Ctx {
    Paragraph,
    Heading(HeadingLevel),
    /// `ordered` + current item index (1-based).
    List { ordered: bool, next_index: u64 },
    Item,
    CodeBlock,
    BlockQuote,
    Link { url: String },
    Emphasis,
    Strong,
    Strikethrough,
    Other,
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    /// Spans being assembled for the current line.
    cur: Vec<Span<'static>>,
    ctx_stack: Vec<Ctx>,
    style_stack: Vec<InlineStyle>,
    style: InlineStyle,
    /// Raw text buffer for fenced/indented code blocks.
    code_buf: String,
    list_depth: usize,
}

impl Renderer {
    fn process<'a>(&mut self, parser: impl Iterator<Item = Event<'a>>) {
        for ev in parser {
            match ev {
                Event::Start(tag) => self.start(tag),
                Event::End(tag) => self.end(tag),
                Event::Text(t) => self.text(&t),
                Event::Code(t) => self.inline_code(&t),
                Event::SoftBreak => self.cur.push(Span::raw(" ")),
                Event::HardBreak => self.flush_line(),
                Event::Rule => {
                    self.lines.push(Line::from(vec![Span::styled(
                        "─".repeat(60),
                        Style::default().fg(Color::DarkGray),
                    )]));
                }
                _ => {}
            }
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if !self.cur.is_empty() {
            self.lines.push(Line::from(std::mem::take(&mut self.cur)));
        }
        self.lines
    }

    // -----------------------------------------------------------------------

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.ctx_stack.push(Ctx::Paragraph),
            Tag::Heading { level, .. } => {
                self.ctx_stack.push(Ctx::Heading(level));
                let mut s = self.style.clone();
                s.bold = true;
                self.push_style(s);
            }
            Tag::List(start) => {
                self.list_depth += 1;
                let ordered = start.is_some();
                let next_index = start.unwrap_or(1);
                self.ctx_stack.push(Ctx::List { ordered, next_index });
            }
            Tag::Item => {
                let (ordered, index) = self.peek_list();
                self.ctx_stack.push(Ctx::Item);
                let indent = "  ".repeat(self.list_depth.saturating_sub(1));
                let prefix = if ordered {
                    format!("{indent}{index}. ")
                } else {
                    format!("{indent}• ")
                };
                self.cur.push(Span::raw(prefix));
            }
            Tag::CodeBlock(_) => {
                self.ctx_stack.push(Ctx::CodeBlock);
                self.code_buf.clear();
            }
            Tag::BlockQuote(_) => self.ctx_stack.push(Ctx::BlockQuote),
            Tag::Link { dest_url, .. } => {
                let url = dest_url.to_string();
                self.ctx_stack.push(Ctx::Link { url: url.clone() });
                let mut s = self.style.clone();
                s.link_url = Some(url);
                self.push_style(s);
            }
            Tag::Emphasis => {
                self.ctx_stack.push(Ctx::Emphasis);
                let mut s = self.style.clone();
                s.italic = true;
                self.push_style(s);
            }
            Tag::Strong => {
                self.ctx_stack.push(Ctx::Strong);
                let mut s = self.style.clone();
                s.bold = true;
                self.push_style(s);
            }
            Tag::Strikethrough => {
                self.ctx_stack.push(Ctx::Strikethrough);
                let mut s = self.style.clone();
                s.strikethrough = true;
                self.push_style(s);
            }
            _ => self.ctx_stack.push(Ctx::Other),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        let ctx = self.ctx_stack.pop();
        match tag {
            TagEnd::Paragraph => {
                self.flush_line();
                self.lines.push(Line::from(""));
            }
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush_line();
                self.lines.push(Line::from(""));
            }
            TagEnd::List(_) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                if self.list_depth == 0 {
                    self.lines.push(Line::from(""));
                }
            }
            TagEnd::Item => {
                self.flush_line();
                // Increment the index in the parent List entry.
                for c in self.ctx_stack.iter_mut().rev() {
                    if let Ctx::List { next_index, .. } = c {
                        *next_index += 1;
                        break;
                    }
                }
            }
            TagEnd::CodeBlock => {
                for line in self.code_buf.lines() {
                    self.lines.push(Line::from(vec![Span::styled(
                        line.to_owned(),
                        Style::default().fg(Color::Cyan),
                    )]));
                }
                self.lines.push(Line::from(""));
                self.code_buf.clear();
            }
            TagEnd::Link => {
                if let Some(Ctx::Link { url }) = ctx
                    && !url.is_empty()
                {
                    self.cur.push(Span::styled(
                        format!(" ({url})"),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::DIM),
                    ));
                }
                self.pop_style();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.pop_style();
            }
            _ => {}
        }
    }

    fn text(&mut self, t: &str) {
        if self.in_code_block() {
            self.code_buf.push_str(t);
            return;
        }
        let prefix = if self.in_block_quote() { "│ " } else { "" };
        let content = if prefix.is_empty() {
            t.to_owned()
        } else {
            format!("{prefix}{t}")
        };
        let style = if let Some(level) = self.heading_level() {
            heading_color(level)
        } else {
            self.style.to_ratatui()
        };
        self.cur.push(Span::styled(content, style));
    }

    fn inline_code(&mut self, t: &str) {
        self.cur.push(Span::styled(
            t.to_owned(),
            Style::default().fg(Color::Cyan),
        ));
    }

    fn flush_line(&mut self) {
        if self.cur.is_empty() {
            return;
        }
        self.lines
            .push(Line::from(std::mem::take(&mut self.cur)));
    }

    fn push_style(&mut self, s: InlineStyle) {
        self.style_stack.push(self.style.clone());
        self.style = s;
    }

    fn pop_style(&mut self) {
        if let Some(prev) = self.style_stack.pop() {
            self.style = prev;
        }
    }

    fn in_code_block(&self) -> bool {
        self.ctx_stack.iter().any(|c| matches!(c, Ctx::CodeBlock))
    }

    fn in_block_quote(&self) -> bool {
        self.ctx_stack.iter().any(|c| matches!(c, Ctx::BlockQuote))
    }

    fn heading_level(&self) -> Option<HeadingLevel> {
        self.ctx_stack.iter().rev().find_map(|c| {
            if let Ctx::Heading(l) = c { Some(*l) } else { None }
        })
    }

    fn peek_list(&self) -> (bool, u64) {
        self.ctx_stack.iter().rev().find_map(|c| {
            if let Ctx::List { ordered, next_index } = c {
                Some((*ordered, *next_index))
            } else {
                None
            }
        }).unwrap_or((false, 1))
    }
}

fn heading_color(level: HeadingLevel) -> Style {
    let color = match level {
        HeadingLevel::H1 => Color::LightYellow,
        HeadingLevel::H2 => Color::LightGreen,
        HeadingLevel::H3 => Color::LightCyan,
        _ => Color::White,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn all_text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn plain_paragraph_renders_as_line() {
        let lines = md_to_lines("Hello world");
        assert!(all_text(&lines).contains("Hello world"));
    }

    #[test]
    fn bold_produces_bold_modifier() {
        let lines = md_to_lines("**bold text**");
        let has_bold = lines
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD));
        assert!(has_bold, "expected a bold span");
    }

    #[test]
    fn italic_produces_italic_modifier() {
        let lines = md_to_lines("*italic text*");
        let has_italic = lines
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| s.style.add_modifier.contains(Modifier::ITALIC));
        assert!(has_italic, "expected an italic span");
    }

    #[test]
    fn inline_code_uses_cyan_fg() {
        let lines = md_to_lines("use `cargo check`");
        let has_cyan = lines
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| s.style.fg == Some(Color::Cyan));
        assert!(has_cyan, "expected cyan inline-code span");
    }

    #[test]
    fn h1_renders_bold_light_yellow() {
        let lines = md_to_lines("# Title");
        let has_h1 = lines.iter().flat_map(|l| &l.spans).any(|s| {
            s.style.add_modifier.contains(Modifier::BOLD)
                && s.style.fg == Some(Color::LightYellow)
        });
        assert!(has_h1, "expected bold LightYellow H1 span");
    }

    #[test]
    fn h2_renders_light_green() {
        let lines = md_to_lines("## Section");
        let has_h2 = lines
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| s.style.fg == Some(Color::LightGreen));
        assert!(has_h2, "expected LightGreen H2 span");
    }

    #[test]
    fn unordered_list_emits_bullet_prefix() {
        let lines = md_to_lines("- item one\n- item two");
        assert!(
            all_text(&lines).contains('•'),
            "expected bullet in list; got: {:?}",
            all_text(&lines)
        );
    }

    #[test]
    fn ordered_list_emits_number_prefix() {
        let lines = md_to_lines("1. first\n2. second");
        let text = all_text(&lines);
        assert!(text.contains("1."), "expected '1.' prefix; got: {text:?}");
    }

    #[test]
    fn code_block_emits_cyan_lines() {
        let md = "```\nfn main() {}\n```";
        let lines = md_to_lines(md);
        let has_code = lines.iter().flat_map(|l| &l.spans).any(|s| {
            s.style.fg == Some(Color::Cyan) && s.content.contains("fn main")
        });
        assert!(has_code, "expected cyan code-block line");
    }

    #[test]
    fn link_appends_url() {
        let lines = md_to_lines("[Rust](https://rust-lang.org)");
        assert!(
            all_text(&lines).contains("rust-lang.org"),
            "expected URL in output"
        );
    }

    #[test]
    fn horizontal_rule_emits_dash_line() {
        let lines = md_to_lines("---");
        let has_rule = lines
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| s.content.contains('─'));
        assert!(has_rule, "expected horizontal rule");
    }

    #[test]
    fn empty_input_produces_no_lines() {
        assert!(md_to_lines("").is_empty());
    }

    // --- split_at_boundary ---

    #[test]
    fn split_no_blank_line_returns_empty_committed() {
        let (c, i) = split_at_boundary("hello world");
        assert_eq!(c, "");
        assert_eq!(i, "hello world");
    }

    #[test]
    fn split_finds_last_blank_line() {
        let text = "para one\n\npara two\n\nin flight";
        let (c, i) = split_at_boundary(text);
        assert!(c.contains("para one"), "committed={c:?}");
        assert!(c.contains("para two"), "committed={c:?}");
        assert_eq!(i, "in flight");
    }

    #[test]
    fn split_single_blank_line() {
        let (c, i) = split_at_boundary("done\n\nstreaming...");
        assert!(c.contains("done"));
        assert_eq!(i, "streaming...");
    }
}
