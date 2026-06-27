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
//! 4. The sanitizer **never** trims or rewrites whitespace outside the
//!    stripped region. Caller-intentional leading / trailing / interior
//!    whitespace — markdown code blocks that begin with `\n`, fenced
//!    examples that end with `\n`, replies with deliberate indentation,
//!    blank-line spacing — is preserved verbatim. Only the bytes between
//!    the `<think>` and `</think>` markers (inclusive of the markers
//!    themselves) are removed. If the model emitted a separator newline
//!    on either side of the block it remains in the output, exactly as
//!    the model wrote it.
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

    SanitizationOutcome {
        text: out,
        stripped_blocks: stripped,
    }
}

const OPEN_TAG: &str = "<think>";
const CLOSE_TAG: &str = "</think>";

// ── Stream-aware sanitizer ───────────────────────────────────────────────────

/// State machine for stripping `<think>…</think>` blocks from a sequence of
/// streaming text deltas, where tag boundaries may split across chunk calls.
///
/// ## Usage
///
/// ```rust,ignore
/// let mut san = StreamThinkSanitizer::new();
/// for delta in runtime_chunks {
///     let safe = san.feed(&delta);
///     if !safe.is_empty() { sse_emit(safe); }
/// }
/// let tail = san.flush(); // emit carry after stream ends
/// ```
///
/// ## Carry buffer
///
/// At the end of each chunk, up to `max(OPEN_TAG.len(), CLOSE_TAG.len()) - 1`
/// bytes are held back if they could be the start of a tag that continues in
/// the next chunk.  `flush()` releases those bytes (or discards them if the
/// stream ended inside a `<think>` block).
pub struct StreamThinkSanitizer {
    state: StreamSanitizerState,
    /// Bytes held from the previous chunk that could be a tag prefix.
    carry: String,
    /// Normal-state prefix not yet emitted: held until the next chunk or
    /// `flush` confirms it is visible, or an orphan `</think>` discards it.
    pending_prefix: String,
    stripped_blocks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamSanitizerState {
    Normal,
    InThink,
}

impl Default for StreamThinkSanitizer {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamThinkSanitizer {
    pub fn new() -> Self {
        Self {
            state: StreamSanitizerState::Normal,
            carry: String::new(),
            pending_prefix: String::new(),
            stripped_blocks: 0,
        }
    }

    /// Feed one streaming delta. Returns the sanitized fragment safe to emit.
    ///
    /// An empty return does not mean the stream is done — it means this chunk
    /// was fully inside a `<think>` block (or held as carry for the next
    /// chunk).  Keep calling `feed` for subsequent chunks.
    pub fn feed(&mut self, chunk: &str) -> String {
        let mut working = std::mem::take(&mut self.pending_prefix);
        if self.carry.is_empty() {
            working.push_str(chunk);
        } else {
            working.push_str(&std::mem::take(&mut self.carry));
            working.push_str(chunk);
        };

        let mut out = String::with_capacity(working.len());
        let mut cursor = 0usize;

        loop {
            let remaining = &working[cursor..];
            if remaining.is_empty() {
                break;
            }

            match self.state {
                StreamSanitizerState::Normal => {
                    let hold = max_tag_prefix_suffix(
                        remaining.as_bytes(),
                        &[OPEN_TAG.as_bytes(), CLOSE_TAG.as_bytes()],
                    );
                    let safe_len = remaining.len() - hold;
                    let scannable = &remaining[..safe_len];

                    match find_next_marker(scannable) {
                        Some(NextMarker::Open(idx)) => {
                            out.push_str(&scannable[..idx]);
                            cursor += idx + OPEN_TAG.len();
                            self.state = StreamSanitizerState::InThink;
                        }
                        Some(NextMarker::Close(idx)) => {
                            // Orphan </think> in Normal state: drop the prefix
                            // before the closer in this buffer (batch sanitizer
                            // contract), including any deferred prefix held from
                            // prior chunks without markers.
                            let _prefix_discarded = &scannable[..idx];
                            self.stripped_blocks += 1;
                            cursor += idx + CLOSE_TAG.len();
                        }
                        None => {
                            let held_suffix = &remaining.as_bytes()[safe_len..];
                            let held_suffix_may_be_close = !held_suffix.is_empty()
                                && CLOSE_TAG.as_bytes()[..held_suffix.len()]
                                    .eq_ignore_ascii_case(held_suffix);
                            if self.stripped_blocks > 0 || (hold > 0 && !held_suffix_may_be_close) {
                                // Confirmed visible: either we have already
                                // crossed a real marker in this stream, or the
                                // held suffix can only become an opener, making
                                // the text before it visible by construction.
                                out.push_str(scannable);
                            } else {
                                // Ambiguous Normal-state text may still precede
                                // a delayed orphan `</think>` in a later chunk.
                                // Keep it quarantined until a later opener
                                // proves it is visible, a closer discards it,
                                // or final `flush()` proves the stream ended
                                // without an orphan closer.
                                self.pending_prefix.push_str(scannable);
                            }
                            self.carry = remaining[safe_len..].to_owned();
                            break;
                        }
                    }
                }
                StreamSanitizerState::InThink => {
                    let hold = max_tag_prefix_suffix(
                        remaining.as_bytes(),
                        &[CLOSE_TAG.as_bytes()],
                    );
                    let safe_len = remaining.len() - hold;
                    let scannable = &remaining[..safe_len];

                    match find_close(scannable) {
                        Some(idx) => {
                            self.stripped_blocks += 1;
                            cursor += idx + CLOSE_TAG.len();
                            self.state = StreamSanitizerState::Normal;
                        }
                        None => {
                            // Still inside think block. Hold potential partial closer.
                            self.carry = remaining[safe_len..].to_owned();
                            break;
                        }
                    }
                }
            }
        }

        out
    }

    /// Flush carry bytes at end of stream.
    ///
    /// - `Normal` state: carry bytes were held as a potential tag prefix but
    ///   no continuation arrived — they are safe model output, return them.
    /// - `InThink` state: carry is inside an unclosed `<think>` block —
    ///   discard (orphan opener, same semantics as `sanitize_assistant_response`).
    pub fn flush(&mut self) -> String {
        let carry = std::mem::take(&mut self.carry);
        let pending = std::mem::take(&mut self.pending_prefix);
        match self.state {
            StreamSanitizerState::Normal => {
                let mut tail = pending;
                tail.push_str(&carry);
                tail
            }
            StreamSanitizerState::InThink => {
                if !carry.is_empty() {
                    self.stripped_blocks += 1;
                }
                String::new()
            }
        }
    }

    /// Number of `<think>` regions stripped so far (updated by `feed`/`flush`).
    pub fn stripped_blocks(&self) -> usize {
        self.stripped_blocks
    }
}

/// Returns the length of the longest suffix of `haystack` that is a proper
/// (non-full) prefix of any tag in `tags` (case-insensitive ASCII comparison).
///
/// "Proper prefix" means shorter than the full tag — a full match is already
/// detectable by `find_ci` and does not need to be carried forward.
fn max_tag_prefix_suffix(haystack: &[u8], tags: &[&[u8]]) -> usize {
    let max_tag_len = tags.iter().map(|t| t.len()).max().unwrap_or(0);
    if max_tag_len <= 1 {
        return 0;
    }
    let check_up_to = (max_tag_len - 1).min(haystack.len());
    // Longest match wins — iterate from longest suffix down.
    for len in (1..=check_up_to).rev() {
        let suffix = &haystack[haystack.len() - len..];
        for &tag in tags {
            if len < tag.len() && tag[..len].eq_ignore_ascii_case(suffix) {
                return len;
            }
        }
    }
    0
}

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
        // The closer's trailing `\n` is the model's separator and remains
        // in the output verbatim — the sanitizer only removes the bytes
        // between the markers (inclusive), never adjacent whitespace.
        let out = sanitize_assistant_response(
            "<think>\nfirst line\nsecond line\n</think>\nactual reply",
        );
        assert_eq!(out.text, "\nactual reply");
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
    fn surrounding_newlines_survive_strip() {
        // The model often wraps the block in newlines for readability.
        // Both newlines belong to the operator-visible byte stream — the
        // sanitizer must not eat them, only the bytes between `<think>`
        // and `</think>` (inclusive of the markers).
        let out = sanitize_assistant_response(
            "prefix line\n<think>hidden</think>\nvisible line",
        );
        assert_eq!(out.text, "prefix line\n\nvisible line");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn preserves_leading_newline_when_caller_intends_it() {
        // Codex review fix: a markdown code-block reply that starts with
        // a deliberate `\n\n` (so the fence renders on its own line)
        // must keep both newlines — they are operator content the
        // sanitizer has no authority to rewrite.
        let input = "<think>plan it</think>\n\n```rust\nfn main() {}\n```\n";
        let out = sanitize_assistant_response(input);
        assert_eq!(out.text, "\n\n```rust\nfn main() {}\n```\n");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn preserves_pure_outer_whitespace_when_no_strip() {
        // No `<think>` at all → output is byte-identical to input. This
        // is the Codex-review fast-path: intentional leading / trailing
        // whitespace must not be trimmed just because the sanitizer was
        // invoked.
        let input = "  \n  hello  \n  ";
        let out = sanitize_assistant_response(input);
        assert_eq!(out.text, input);
        assert_eq!(out.stripped_blocks, 0);
    }

    #[test]
    fn preserves_trailing_whitespace_after_strip() {
        // Trailing spaces *after* the strip boundary are operator
        // content (e.g., a model that pads with spaces for line-buffer
        // flush). They must survive.
        let out = sanitize_assistant_response("<think>r</think>visible  ");
        assert_eq!(out.text, "visible  ");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn preserves_leading_whitespace_before_strip() {
        // Indentation before the block (e.g., a markdown blockquote
        // continuation) is operator content. The sanitizer removes only
        // the block, not the indentation.
        let out = sanitize_assistant_response("  <think>r</think>line");
        assert_eq!(out.text, "  line");
        assert_eq!(out.stripped_blocks, 1);
    }

    #[test]
    fn preserves_crlf_line_endings_around_block() {
        // Windows-style `\r\n` separators around the block are operator
        // content too — they remain in the output verbatim.
        let out = sanitize_assistant_response(
            "prefix\r\n<think>r</think>\r\nsuffix",
        );
        assert_eq!(out.text, "prefix\r\n\r\nsuffix");
        assert_eq!(out.stripped_blocks, 1);
    }

    // ── StreamThinkSanitizer tests ──────────────────────────────────────────

    #[test]
    fn stream_passthrough_when_no_markers() {
        let mut s = StreamThinkSanitizer::new();
        // First chunk with no markers is held until flush (one-chunk latency).
        assert_eq!(s.feed("hello world"), "");
        assert_eq!(s.flush(), "hello world");
        assert_eq!(s.stripped_blocks(), 0);
    }

    #[test]
    fn stream_strips_complete_block_single_chunk() {
        let mut s = StreamThinkSanitizer::new();
        assert_eq!(s.feed("<think>secret</think>visible"), "visible");
        assert_eq!(s.stripped_blocks(), 1);
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn stream_open_tag_split_across_chunks() {
        // <think> split: chunk 1 ends with '<thi', chunk 2 completes it.
        let mut s = StreamThinkSanitizer::new();
        let out1 = s.feed("before<thi");
        assert_eq!(out1, "before");
        let out2 = s.feed("nk>secret</think>after");
        assert_eq!(out2, "after");
        assert_eq!(s.flush(), "");
        assert_eq!(s.stripped_blocks(), 1);
    }

    #[test]
    fn stream_close_tag_split_across_chunks() {
        // </think> split: chunk 1 ends with '</thi', chunk 2 completes it.
        let mut s = StreamThinkSanitizer::new();
        let out1 = s.feed("<think>secret</thi");
        assert_eq!(out1, "");
        let out2 = s.feed("nk>visible");
        assert_eq!(out2, "visible");
        assert_eq!(s.stripped_blocks(), 1);
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn stream_open_tag_split_one_byte_at_a_time() {
        // Worst-case: each byte of <think> arrives in its own chunk.
        let mut s = StreamThinkSanitizer::new();
        for ch in ["<", "t", "h", "i", "n", "k", ">"] {
            let out = s.feed(ch);
            assert_eq!(out, "", "expected empty while tag is building, got {out:?}");
        }
        assert_eq!(s.feed("content</think>done"), "done");
        assert_eq!(s.stripped_blocks(), 1);
    }

    #[test]
    fn stream_multiple_blocks_split_differently() {
        let mut s = StreamThinkSanitizer::new();
        // Block 1 split at open tag, block 2 split at close tag.
        let out1 = s.feed("<think>a</");
        assert_eq!(out1, "");
        let out2 = s.feed("think>one<think>b<");
        assert_eq!(out2, "one");
        let out3 = s.feed("/think>two");
        assert_eq!(out3, "two");
        assert_eq!(s.stripped_blocks(), 2);
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn stream_flush_in_normal_state_emits_carry() {
        // Stream ends with bytes that looked like a tag start but aren't.
        let mut s = StreamThinkSanitizer::new();
        let out = s.feed("text<thi");
        assert_eq!(out, "text");
        // '<thi' held as carry; flush releases it (not a real tag).
        assert_eq!(s.flush(), "<thi");
        assert_eq!(s.stripped_blocks(), 0);
    }

    #[test]
    fn stream_flush_in_think_state_discards_carry() {
        // Unclosed <think> at end of stream: content must be dropped.
        let mut s = StreamThinkSanitizer::new();
        let out = s.feed("<think>hidden content");
        assert_eq!(out, "");
        // InThink state: flush discards.
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn stream_case_insensitive() {
        let mut s = StreamThinkSanitizer::new();
        assert_eq!(s.feed("<THINK>hidden</THINK>visible"), "visible");
        assert_eq!(s.stripped_blocks(), 1);
    }

    #[test]
    fn stream_single_byte_chunks_close_tag_split() {
        let input = "<think>secret</think>reply";
        let mut s = StreamThinkSanitizer::new();
        let mut combined = String::new();
        for byte in input.as_bytes() {
            combined.push_str(&s.feed(&(*byte as char).to_string()));
        }
        combined.push_str(&s.flush());
        assert_eq!(combined, "reply");
        assert_eq!(s.stripped_blocks(), 1);
    }

    #[test]
    fn stream_result_matches_batch_sanitizer_for_balanced_blocks() {
        // For balanced <think>…</think> blocks and unclosed openers, the
        // concatenation of stream deltas + flush equals the batch result.
        //
        // Orphan closers match the batch sanitizer when the prefix and closer
        // land in the same scannable buffer (e.g. whole input fed in one chunk),
        // or when a deferred prefix is discarded before a delayed orphan closer
        // (`stream_orphan_closer_split_across_chunks_deferred_prefix_not_leaked`).
        let inputs = [
            "<think>a</think>text",
            "no markers here",
            "<think>a</think><think>b</think>x",
            "<think>unclosed",
            "<thi",
        ];
        for input in inputs {
            let batch = sanitize_assistant_response(input);
            let mut s = StreamThinkSanitizer::new();
            // Feed in 3-byte chunks to exercise split-tag paths.
            let mut streamed = String::new();
            let mut idx = 0;
            while idx < input.len() {
                let end = (idx + 3).min(input.len());
                streamed.push_str(&s.feed(&input[idx..end]));
                idx = end;
            }
            streamed.push_str(&s.flush());
            assert_eq!(
                streamed, batch.text,
                "stream vs batch mismatch for input: {input:?}"
            );
        }
    }

    #[test]
    fn stream_many_orphan_closers_terminates() {
        let mut s = StreamThinkSanitizer::new();
        let junk = "</think>".repeat(100);
        let out = s.feed(&junk);
        assert!(out.is_empty());
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn stream_orphan_closer_drops_prefix_in_buffer() {
        // Orphan closer in a single chunk: prefix before `</think>` must not
        // be emitted, matching `sanitize_assistant_response`.
        let mut s = StreamThinkSanitizer::new();
        let out = s.feed("leaked reasoning</think>actual reply");
        assert_eq!(out, "actual reply");
        assert_eq!(s.stripped_blocks(), 1);
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn stream_orphan_closer_matches_batch_when_whole_input_in_one_chunk() {
        let input = "hidden reasoning</think>visible";
        let batch = sanitize_assistant_response(input);
        let mut s = StreamThinkSanitizer::new();
        let streamed = format!("{}{}", s.feed(input), s.flush());
        assert_eq!(streamed, batch.text);
        assert_eq!(s.stripped_blocks(), batch.stripped_blocks);
    }

    #[test]
    fn stream_orphan_closer_split_across_chunks_deferred_prefix_not_leaked() {
        // Mid-think stream start: first chunk has no marker; orphan closer in
        // the second chunk must discard the deferred prefix (P1 regression).
        let mut s = StreamThinkSanitizer::new();
        assert_eq!(s.feed("hidden reasoning"), "");
        assert_eq!(s.feed("</think>visible"), "visible");
        assert_eq!(s.stripped_blocks(), 1);
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn stream_markerless_prefix_stays_quarantined_until_flush() {
        let mut s = StreamThinkSanitizer::new();
        assert_eq!(s.feed("hello"), "");
        assert_eq!(s.feed(" world"), "");
        assert_eq!(s.flush(), "hello world");
    }

    #[test]
    fn stream_orphan_closer_after_multiple_markerless_chunks_discards_all_prefix() {
        // Codex PR #1328 P1 regression: a stream that starts mid-think can
        // span several markerless chunks before the orphan closer arrives.
        // None of that pre-closer text may be emitted early.
        let mut s = StreamThinkSanitizer::new();
        assert_eq!(s.feed("hidden "), "");
        assert_eq!(s.feed("reasoning "), "");
        assert_eq!(s.feed("details</think>visible"), "visible");
        assert_eq!(s.stripped_blocks(), 1);
        assert_eq!(s.flush(), "");
    }

    #[test]
    fn max_tag_prefix_suffix_finds_correct_lengths() {
        let tags: &[&[u8]] = &[OPEN_TAG.as_bytes(), CLOSE_TAG.as_bytes()];
        // Full suffix matching open tag prefix
        assert_eq!(max_tag_prefix_suffix(b"abc<thi", tags), 4);
        // Single < could start either tag
        assert_eq!(max_tag_prefix_suffix(b"abc<", tags), 1);
        // No suffix matches
        assert_eq!(max_tag_prefix_suffix(b"abcxyz", tags), 0);
        // Close tag prefix
        assert_eq!(max_tag_prefix_suffix(b"abc</thi", tags), 5);
        // Full tag at end is NOT a partial (it's a full match, no carry needed)
        assert_eq!(max_tag_prefix_suffix(b"abc<think>", tags), 0);
    }
}
