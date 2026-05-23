//! Operator-facing response sanitizer (Hermes parity matrix Row 4).
//!
//! Reasoning models — Qwen, DeepSeek-R1, GLM-4.6, etc. — emit raw
//! chain-of-thought wrapped in `<think>…</think>` blocks alongside the
//! actual assistant reply. SERA's operator-visible `/api/chat` `response`
//! field must never leak these blocks: they are not part of the assistant
//! contract, they expose internal reasoning the user did not ask for, and
//! the parity baseline gate
//! (`rust/crates/sera-e2e-harness/tests/hermes_parity_baseline.rs`) treats
//! a leak as a hard regression.
//!
//! This module enforces that contract at the gateway boundary. The runtime
//! may keep the raw text in logs / transcript / audit for debugging, but
//! the bytes the operator actually sees pass through
//! [`sanitize_assistant_response`] first.
//!
//! ## Stripping rules
//!
//! 1. Balanced `<think>…</think>` pairs are removed in full, including the
//!    tags themselves. Case-insensitive (`<Think>`, `<THINK>`, … all match).
//! 2. An orphan `<think>` with no closing tag swallows everything from the
//!    opening tag to end-of-string. Truncated streams are the common cause;
//!    leaking the rest would defeat the purpose.
//! 3. An orphan `</think>` with no opening tag swallows everything from the
//!    start of the string up to and including the closing tag — the prefix
//!    is assumed to be hidden reasoning that lost its opening marker.
//! 4. Leading / trailing whitespace introduced *by* stripping is trimmed,
//!    but interior whitespace is preserved verbatim.
//!
//! The sanitizer is intentionally line-and-byte agnostic: `<think>` tags
//! can span newlines, can be adjacent to assistant text on the same line,
//! and can appear multiple times in one reply.

/// Outcome of a single sanitization pass.
///
/// `text` is always safe to hand to an operator; `stripped_blocks` is the
/// number of `<think>` regions removed (counting both balanced pairs and
/// orphan halves). Callers use the count for audit / metrics so a
/// reasoning-model regression is observable rather than silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizationOutcome {
    pub text: String,
    pub stripped_blocks: usize,
}

impl SanitizationOutcome {
    /// True when at least one `<think>` region was removed.
    pub fn was_sanitized(&self) -> bool {
        self.stripped_blocks > 0
    }
}

/// Strip `<think>…</think>` blocks from an assistant-visible reply.
///
/// See the module docs for the full contract. The input is borrowed; the
/// returned `text` is a freshly owned `String` even when no stripping was
/// needed (so callers can hand it onward without lifetime juggling).
pub fn sanitize_assistant_response(input: &str) -> SanitizationOutcome {
    // Fast path: the overwhelming majority of replies (non-reasoning
    // models, mock LLMs, cached replies) carry no `<think>` markers at
    // all. Avoid the allocation and scan in that case.
    if !contains_think_marker(input) {
        return SanitizationOutcome {
            text: input.to_owned(),
            stripped_blocks: 0,
        };
    }

    let mut out = String::with_capacity(input.len());
    let mut stripped = 0usize;
    let mut cursor = 0usize;

    loop {
        let rest = &input[cursor..];
        match find_next_marker(rest) {
            Some(NextMarker::Open(rel_idx)) => {
                let abs_idx = cursor + rel_idx;
                out.push_str(&input[cursor..abs_idx]);
                // Tag length is fixed regardless of case (`<think>` = 7).
                let after_open = abs_idx + OPEN_TAG.len();
                match find_close(&input[after_open..]) {
                    Some(rel_close) => {
                        let close_start = after_open + rel_close;
                        let close_end = close_start + CLOSE_TAG.len();
                        stripped += 1;
                        cursor = close_end;
                    }
                    None => {
                        // Orphan opener — swallow the remainder. A truncated
                        // stream is the common cause; leaking the partial
                        // chain-of-thought would defeat the contract.
                        stripped += 1;
                        cursor = input.len();
                        break;
                    }
                }
            }
            Some(NextMarker::Close(rel_idx)) => {
                // Orphan closer with no opener seen yet at `cursor`. Treat
                // the prefix as hidden reasoning that lost its opening
                // marker and drop everything up to and including the
                // closing tag.
                let abs_idx = cursor + rel_idx;
                stripped += 1;
                cursor = abs_idx + CLOSE_TAG.len();
            }
            None => break,
        }
    }
    if cursor < input.len() {
        out.push_str(&input[cursor..]);
    }

    // Stripping a block surrounded by whitespace commonly leaves a leading
    // newline or trailing dangling space; trim only the outer edges so
    // interior formatting (lists, code fences) is preserved.
    let trimmed = out.trim().to_owned();

    SanitizationOutcome {
        text: trimmed,
        stripped_blocks: stripped,
    }
}

const OPEN_TAG: &str = "<think>";
const CLOSE_TAG: &str = "</think>";

enum NextMarker {
    Open(usize),
    Close(usize),
}

fn contains_think_marker(haystack: &str) -> bool {
    find_ci(haystack, OPEN_TAG).is_some() || find_ci(haystack, CLOSE_TAG).is_some()
}

fn find_next_marker(haystack: &str) -> Option<NextMarker> {
    match (find_ci(haystack, OPEN_TAG), find_ci(haystack, CLOSE_TAG)) {
        (Some(o), Some(c)) if o <= c => Some(NextMarker::Open(o)),
        (Some(_), Some(c)) => Some(NextMarker::Close(c)),
        (Some(o), None) => Some(NextMarker::Open(o)),
        (None, Some(c)) => Some(NextMarker::Close(c)),
        (None, None) => None,
    }
}

fn find_close(haystack: &str) -> Option<usize> {
    find_ci(haystack, CLOSE_TAG)
}

/// Case-insensitive substring search constrained to the ASCII tag bytes we
/// care about. The `needle` is always ASCII (`<think>` / `</think>`), so we
/// can compare byte-wise without worrying about UTF-8 boundaries on the
/// needle side; the haystack may contain multibyte UTF-8 but the windowed
/// match only succeeds when every byte in the window is ASCII, which keeps
/// us on char boundaries.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    debug_assert!(needle.is_ascii());
    let needle = needle.as_bytes();
    let hay = haystack.as_bytes();
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    'outer: for i in 0..=last {
        for j in 0..needle.len() {
            if !hay[i + j].eq_ignore_ascii_case(&needle[j]) {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_no_marker() {
        let out = sanitize_assistant_response("hello world");
        assert_eq!(out.text, "hello world");
        assert_eq!(out.stripped_blocks, 0);
        assert!(!out.was_sanitized());
    }

    #[test]
    fn strips_single_balanced_block() {
        let out =
            sanitize_assistant_response("<think>secret reasoning</think>visible reply");
        assert_eq!(out.text, "visible reply");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn strips_block_at_end() {
        let out =
            sanitize_assistant_response("visible reply<think>secret</think>");
        assert_eq!(out.text, "visible reply");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn strips_multiple_blocks() {
        let out = sanitize_assistant_response(
            "<think>a</think>one<think>b</think>two<think>c</think>three",
        );
        assert_eq!(out.text, "onetwothree");
        assert_eq!(out.stripped_blocks, 3);
    }

    #[test]
    fn strips_multiline_block() {
        let out = sanitize_assistant_response(
            "<think>\nfirst line\nsecond line\n</think>\nactual reply",
        );
        assert_eq!(out.text, "actual reply");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn case_insensitive_tags() {
        let cases = [
            "<Think>x</Think>after",
            "<THINK>x</THINK>after",
            "<thInK>x</tHinK>after",
        ];
        for input in cases {
            let out = sanitize_assistant_response(input);
            assert_eq!(out.text, "after", "input={input:?}");
            assert_eq!(out.stripped_blocks, 1);
        }
    }

    #[test]
    fn orphan_opener_swallows_remainder() {
        let out = sanitize_assistant_response(
            "visible<think>truncated chain of thought ends here",
        );
        assert_eq!(out.text, "visible");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn orphan_closer_drops_prefix() {
        let out = sanitize_assistant_response(
            "leaked reasoning</think>actual reply",
        );
        assert_eq!(out.text, "actual reply");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn preserves_interior_whitespace_and_unicode() {
        let out = sanitize_assistant_response(
            "<think>r</think>line 1\n\nline 2\n  - bullet ✓\nαβγ",
        );
        assert_eq!(out.text, "line 1\n\nline 2\n  - bullet ✓\nαβγ");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn empty_input_is_passthrough() {
        let out = sanitize_assistant_response("");
        assert_eq!(out.text, "");
        assert_eq!(out.stripped_blocks, 0);
    }

    #[test]
    fn empty_think_block_still_counts() {
        // Models can emit an empty `<think></think>` when no chain was
        // produced; treat as a stripped block so downstream metrics
        // still surface it.
        let out = sanitize_assistant_response("<think></think>reply");
        assert_eq!(out.text, "reply");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn outcome_text_only_when_pure_think_block() {
        let out = sanitize_assistant_response("<think>only reasoning</think>");
        assert_eq!(out.text, "");
        assert_eq!(out.stripped_blocks, 1);
        assert!(out.was_sanitized());
    }

    #[test]
    fn does_not_strip_unrelated_angle_bracket_text() {
        // Generic `<foo>` content must not be touched — only `<think>`.
        let out =
            sanitize_assistant_response("here is <code>x</code> and <pre>y</pre>");
        assert_eq!(out.text, "here is <code>x</code> and <pre>y</pre>");
        assert_eq!(out.stripped_blocks, 0);
    }

    #[test]
    fn does_not_match_thinking_substring() {
        // The tag is exactly `<think>`; the word "thinking" inside other
        // markup must not be mistaken for an opener.
        let out = sanitize_assistant_response(
            "I am thinking about <think>secret</think>visible",
        );
        assert_eq!(out.text, "I am thinking about visible");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn handles_block_surrounded_by_whitespace() {
        let out = sanitize_assistant_response(
            "  \n<think>r</think>\n  visible \n",
        );
        // Outer whitespace trimmed; interior preserved.
        assert_eq!(out.text, "visible");
        assert_eq!(out.stripped_blocks, 1);
    }
}
