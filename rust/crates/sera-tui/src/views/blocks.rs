//! Block-based transcript model — **J.0.2 (sera-s3gb)**.
//!
//! Replaces the flat `Vec<TranscriptEntry>` + `Vec<String>` tool log used in
//! waves G/J.0.1 with a typed [`Block`] enum.  Each variant maps to one
//! transcript entry the renderer can style independently:
//!
//! * `UserMessage` / `AssistantMessage` — chat text.  Markdown is stubbed
//!   as a plain `String` here; J.1.1 (sera-7fpx) wires `pulldown-cmark`
//!   parsing and replaces the field with a parsed-tree shape.
//! * `ToolCall` — collapsed by default (`expanded=false`).  J.0.3
//!   (sera-c6mb) hooks `Space` to toggle expansion; the renderer reads the
//!   field directly.
//! * `Task` — subagent thread placeholder.  J.2.x lands the child thread
//!   drill-in; for J.0.2 the variant is shaped but unrendered.
//! * `Approval` — inline HITL approval; J.1.4 (sera-7olp) promotes the
//!   modal into this block.
//! * `Error` — inline error banner.
//! * `TurnSeparator` — emitted at end-of-turn so renderers can insert a
//!   visual break between turns.
//!
//! ## Streaming semantics
//!
//! [`Block::extend_assistant_text`] appends a delta to the **trailing**
//! `AssistantMessage` block when one exists; otherwise it pushes a new
//! block.  The contract matches the autonomous gateway's emit pattern
//! (one assistant message per turn) and is the same single-turn
//! assumption the previous `apply_event` reducer used — only the storage
//! shape changes.

use serde_json::Value;

/// One entry in the transcript.  Variants are kept flat (no `Box`) since
/// the largest payload is `Value` (already heap-allocated by serde_json).
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A message the operator typed into the composer.
    UserMessage { text: String },
    /// An assistant turn.  `text` is plain markdown source; J.1.1 will
    /// add a parsed-tree field alongside.  The `streaming` flag marks
    /// the in-flight block so renderers can draw a cursor / spinner.
    AssistantMessage { text: String, streaming: bool },
    /// A tool invocation.  Collapsed by default (`expanded=false`).
    /// `summary` is the one-line label after the tool name (`Write(poem.md)`).
    /// `args` is the raw argument JSON; `result` is `None` until the
    /// matching `tool_end` event arrives.
    ToolCall {
        tool: String,
        summary: String,
        args: Value,
        result: Option<ToolResult>,
        expanded: bool,
    },
    /// A subagent task block.  Shape only in J.0.2; J.2.1 lands the
    /// child `ThreadView` and the drill-in stack.
    #[allow(dead_code)]
    Task {
        agent: String,
        summary: String,
        expanded: bool,
    },
    /// An inline HITL approval request.  Promoted from the modal in J.1.4.
    Approval {
        request_id: String,
        tool: String,
        reason: String,
        status: ApprovalStatus,
    },
    /// An inline error banner — typically a stream error or a non-retryable
    /// gateway response.
    Error { message: String, retryable: bool },
    /// A visual separator the renderer draws between turns.  Pushed when a
    /// `turn_completed` SSE frame arrives.
    TurnSeparator,
}

/// Result payload attached to a [`Block::ToolCall`] when the tool finishes.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub ok: bool,
    pub output: String,
}

/// Status of an inline approval block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    /// Wired by J.1.4 (sera-7olp) when the operator presses `[a]pprove`.
    #[allow(dead_code)]
    Approved,
    /// Wired by J.1.4 (sera-7olp) when the operator presses `[r]eject`.
    #[allow(dead_code)]
    Rejected,
    /// Wired by J.1.4 (sera-7olp) when the operator presses `[e]scalate`.
    #[allow(dead_code)]
    Escalated,
}

impl Block {
    /// Append `delta` to the trailing assistant block in `blocks`, or
    /// push a new `AssistantMessage` block if the tail isn't one.
    /// Always leaves the trailing assistant block in `streaming=true`.
    /// Returns `true` if a block was modified or appended.
    pub fn extend_assistant_text(blocks: &mut Vec<Block>, delta: &str) -> bool {
        if delta.is_empty() {
            return false;
        }
        match blocks.last_mut() {
            Some(Block::AssistantMessage { text, streaming }) => {
                text.push_str(delta);
                *streaming = true;
            }
            _ => blocks.push(Block::AssistantMessage {
                text: delta.to_owned(),
                streaming: true,
            }),
        }
        true
    }

    /// Mark every trailing in-flight assistant block as no longer streaming.
    /// Called when a `turn_completed` event arrives.  Wired by
    /// `SessionView::finalize_turn`; J.0.4 (sera-zate) calls it on cancel.
    #[allow(dead_code)]
    pub fn finalize_streaming(blocks: &mut [Block]) {
        for block in blocks.iter_mut().rev() {
            match block {
                Block::AssistantMessage { streaming, .. } => *streaming = false,
                Block::TurnSeparator => continue,
                _ => break,
            }
        }
    }

    /// Toggle `expanded` if `self` is a tool or task block.  Other variants
    /// are left untouched.  Returns the new expanded state, or `None` for
    /// non-toggleable blocks.  Wired by J.0.3 (sera-c6mb) when Space is
    /// pressed on a focused tool block.
    #[allow(dead_code)]
    pub fn toggle_expanded(&mut self) -> Option<bool> {
        match self {
            Block::ToolCall { expanded, .. } | Block::Task { expanded, .. } => {
                *expanded = !*expanded;
                Some(*expanded)
            }
            _ => None,
        }
    }

    /// Plain-text rendering used by the J.0.2 transitional renderer.
    /// J.1.1 will add a markdown-aware path; this lives here so widgets
    /// don't have to match on every variant.
    pub fn plain_text(&self) -> String {
        match self {
            Block::UserMessage { text } => text.clone(),
            Block::AssistantMessage { text, .. } => text.clone(),
            Block::ToolCall {
                tool,
                summary,
                expanded,
                result,
                args,
            } => {
                let arrow = if *expanded { "▾" } else { "▸" };
                let mut s = format!("⏺ {tool}({summary}) {arrow}");
                if *expanded {
                    s.push_str(&format!("\n  args: {args}"));
                    if let Some(r) = result {
                        let label = if r.ok { "ok" } else { "err" };
                        s.push_str(&format!("\n  result: {label} — {}", r.output));
                    }
                }
                s
            }
            Block::Task {
                agent,
                summary,
                expanded,
            } => {
                let arrow = if *expanded { "▾" } else { "▸" };
                format!("⏺ Task({agent}: {summary}) {arrow}")
            }
            Block::Approval {
                tool,
                reason,
                status,
                ..
            } => format!("⚠ Approval required: {tool} — {reason} [{status:?}]"),
            Block::Error { message, .. } => format!("⨯ {message}"),
            Block::TurnSeparator => String::new(),
        }
    }

    /// Stable role label used by the J.0.2 transitional list renderer to
    /// pick a colour.  Matches the legacy role strings ("user",
    /// "assistant", "tool", "system") so existing snapshot-style tests
    /// keep working.
    #[allow(dead_code)]
    pub fn role_label(&self) -> &'static str {
        match self {
            Block::UserMessage { .. } => "user",
            Block::AssistantMessage { .. } => "assistant",
            Block::ToolCall { .. } => "tool",
            Block::Task { .. } => "task",
            Block::Approval { .. } => "approval",
            Block::Error { .. } => "error",
            Block::TurnSeparator => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extend_assistant_appends_to_trailing_block() {
        let mut blocks = Vec::new();
        Block::extend_assistant_text(&mut blocks, "hello ");
        Block::extend_assistant_text(&mut blocks, "world");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::AssistantMessage { text, streaming } => {
                assert_eq!(text, "hello world");
                assert!(streaming);
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn extend_assistant_pushes_new_block_when_tail_is_user() {
        let mut blocks = vec![Block::UserMessage {
            text: "hi".into(),
        }];
        Block::extend_assistant_text(&mut blocks, "answer");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(
            blocks[1],
            Block::AssistantMessage { ref text, .. } if text == "answer"
        ));
    }

    #[test]
    fn extend_assistant_empty_delta_is_noop() {
        let mut blocks = Vec::new();
        assert!(!Block::extend_assistant_text(&mut blocks, ""));
        assert!(blocks.is_empty());
    }

    #[test]
    fn finalize_streaming_clears_trailing_assistant_flag() {
        let mut blocks = vec![
            Block::UserMessage { text: "q".into() },
            Block::AssistantMessage {
                text: "a".into(),
                streaming: true,
            },
        ];
        Block::finalize_streaming(&mut blocks);
        match &blocks[1] {
            Block::AssistantMessage { streaming, .. } => assert!(!streaming),
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn finalize_streaming_skips_turn_separator_to_reach_assistant() {
        let mut blocks = vec![
            Block::AssistantMessage {
                text: "a".into(),
                streaming: true,
            },
            Block::TurnSeparator,
        ];
        Block::finalize_streaming(&mut blocks);
        match &blocks[0] {
            Block::AssistantMessage { streaming, .. } => assert!(!streaming),
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn toggle_expanded_flips_tool_blocks() {
        let mut b = Block::ToolCall {
            tool: "Write".into(),
            summary: "poem.md".into(),
            args: Value::Null,
            result: None,
            expanded: false,
        };
        assert_eq!(b.toggle_expanded(), Some(true));
        assert_eq!(b.toggle_expanded(), Some(false));
    }

    #[test]
    fn toggle_expanded_returns_none_for_non_toggleable() {
        let mut b = Block::UserMessage { text: "x".into() };
        assert_eq!(b.toggle_expanded(), None);
    }

    #[test]
    fn plain_text_collapsed_tool_uses_arrow() {
        let b = Block::ToolCall {
            tool: "Write".into(),
            summary: "poem.md".into(),
            args: Value::Null,
            result: None,
            expanded: false,
        };
        let s = b.plain_text();
        assert!(s.contains("Write(poem.md)"));
        assert!(s.contains("▸"));
        assert!(!s.contains("▾"));
        assert!(!s.contains("args:"));
    }

    #[test]
    fn plain_text_expanded_tool_includes_args_and_result() {
        let b = Block::ToolCall {
            tool: "Write".into(),
            summary: "poem.md".into(),
            args: serde_json::json!({"path": "poem.md"}),
            result: Some(ToolResult {
                ok: true,
                output: "62 bytes".into(),
            }),
            expanded: true,
        };
        let s = b.plain_text();
        assert!(s.contains("▾"));
        assert!(s.contains("args:"));
        assert!(s.contains("result: ok"));
        assert!(s.contains("62 bytes"));
    }

    #[test]
    fn role_label_matches_legacy_strings() {
        assert_eq!(
            Block::UserMessage { text: "".into() }.role_label(),
            "user"
        );
        assert_eq!(
            Block::AssistantMessage {
                text: "".into(),
                streaming: false,
            }
            .role_label(),
            "assistant"
        );
        assert_eq!(
            Block::ToolCall {
                tool: "".into(),
                summary: "".into(),
                args: Value::Null,
                result: None,
                expanded: false,
            }
            .role_label(),
            "tool"
        );
    }
}
