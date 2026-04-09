//! Inbound debounce buffer — SPEC-gateway §4.2.
//!
//! Humans type in bursts. This buffer collects rapid consecutive text messages
//! for a session and releases them as a single batch once the debounce window
//! has elapsed without a new message arriving.
//!
//! Rules per spec:
//! - Attachments and media flush immediately (caller responsibility — pass them
//!   directly to the queue without going through this buffer).
//! - Control commands (e.g. `/pause`, `/cancel`) bypass debouncing entirely.
//! - Window is configurable globally and per-channel; defaults to 500 ms.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default debounce window (500 ms per spec).
pub const DEFAULT_DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);

/// Per-session buffer state.
#[derive(Debug)]
struct SessionBuffer {
    /// Messages collected since the window opened.
    messages: Vec<String>,
    /// Time the most recent message was pushed into this buffer.
    last_push: Instant,
}

impl SessionBuffer {
    fn new(first_message: String) -> Self {
        Self {
            messages: vec![first_message],
            last_push: Instant::now(),
        }
    }

    fn push(&mut self, message: String) {
        self.messages.push(message);
        self.last_push = Instant::now();
    }

    /// Return true if the debounce window has elapsed since the last push.
    fn is_ready(&self, window: Duration) -> bool {
        self.last_push.elapsed() >= window
    }
}

/// Collects rapid consecutive messages per session and releases them in batches
/// once the debounce window has elapsed.
#[derive(Debug)]
pub struct DebounceBuffer {
    window: Duration,
    sessions: HashMap<String, SessionBuffer>,
}

impl DebounceBuffer {
    /// Create a new buffer with the given debounce window.
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            sessions: HashMap::new(),
        }
    }

    /// Create a buffer with the default 500 ms window.
    pub fn with_default_window() -> Self {
        Self::new(DEFAULT_DEBOUNCE_WINDOW)
    }

    /// Add `message` to the buffer for `session_key`.
    ///
    /// Resets the debounce timer for that session. If no buffer exists yet
    /// for the session one is created.
    pub fn push(&mut self, session_key: &str, message: String) {
        self.sessions
            .entry(session_key.to_string())
            .and_modify(|buf| buf.push(message.clone()))
            .or_insert_with(|| SessionBuffer::new(message));
    }

    /// Attempt to flush the buffer for `session_key`.
    ///
    /// Returns `Some(messages)` if the debounce window has elapsed since the
    /// last push, consuming and returning all buffered messages.
    /// Returns `None` if the window has not yet elapsed (still accumulating).
    pub fn flush(&mut self, session_key: &str) -> Option<Vec<String>> {
        let ready = self
            .sessions
            .get(session_key)
            .map(|buf| buf.is_ready(self.window))
            .unwrap_or(false);

        if ready {
            self.sessions
                .remove(session_key)
                .map(|buf| buf.messages)
        } else {
            None
        }
    }

    /// Flush all sessions whose debounce window has elapsed.
    ///
    /// Returns a `Vec` of `(session_key, messages)` pairs. Sessions that are
    /// still accumulating are left untouched.
    pub fn flush_all_ready(&mut self) -> Vec<(String, Vec<String>)> {
        let window = self.window;
        let ready_keys: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, buf)| buf.is_ready(window))
            .map(|(key, _)| key.clone())
            .collect();

        ready_keys
            .into_iter()
            .filter_map(|key| {
                self.sessions
                    .remove(&key)
                    .map(|buf| (key, buf.messages))
            })
            .collect()
    }

    /// Return the number of sessions currently buffering messages.
    pub fn active_sessions(&self) -> usize {
        self.sessions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    // --- push / flush single session ---

    #[test]
    fn flush_returns_none_before_window_elapses() {
        let mut buf = DebounceBuffer::new(Duration::from_secs(60));
        buf.push("session-1", "hello".to_string());
        // Window has not elapsed — should not flush.
        assert!(buf.flush("session-1").is_none());
    }

    #[test]
    fn flush_returns_messages_after_window_elapses() {
        let window = Duration::from_millis(50);
        let mut buf = DebounceBuffer::new(window);

        buf.push("session-1", "hello".to_string());
        buf.push("session-1", "world".to_string());

        thread::sleep(Duration::from_millis(60));

        let flushed = buf.flush("session-1").expect("should flush after window");
        assert_eq!(flushed, vec!["hello", "world"]);
    }

    #[test]
    fn flush_consumes_the_buffer() {
        let window = Duration::from_millis(50);
        let mut buf = DebounceBuffer::new(window);

        buf.push("s", "msg".to_string());
        thread::sleep(Duration::from_millis(60));

        assert!(buf.flush("s").is_some());
        // Second call should return None — buffer was consumed.
        assert!(buf.flush("s").is_none());
        assert_eq!(buf.active_sessions(), 0);
    }

    // --- batching: new push resets timer ---

    #[test]
    fn new_push_resets_window() {
        let window = Duration::from_millis(80);
        let mut buf = DebounceBuffer::new(window);

        buf.push("s", "first".to_string());
        thread::sleep(Duration::from_millis(50));
        // Push again before window elapses — resets the timer.
        buf.push("s", "second".to_string());
        thread::sleep(Duration::from_millis(50));

        // 50+50 = 100ms total since first push, but last push was only 50ms ago.
        // The window should NOT have elapsed relative to `last_push`.
        // (window is 80ms, 50ms since last push < 80ms)
        assert!(buf.flush("s").is_none());

        // Now wait until window elapses from last push.
        thread::sleep(Duration::from_millis(40));
        let flushed = buf.flush("s").expect("should flush now");
        assert_eq!(flushed, vec!["first", "second"]);
    }

    // --- multiple sessions are independent ---

    #[test]
    fn sessions_are_independent() {
        let window = Duration::from_millis(50);
        let mut buf = DebounceBuffer::new(window);

        buf.push("s1", "msg-a".to_string());
        buf.push("s2", "msg-b".to_string());

        thread::sleep(Duration::from_millis(60));

        let r1 = buf.flush("s1").expect("s1 ready");
        let r2 = buf.flush("s2").expect("s2 ready");

        assert_eq!(r1, vec!["msg-a"]);
        assert_eq!(r2, vec!["msg-b"]);
    }

    // --- flush_all_ready ---

    #[test]
    fn flush_all_ready_returns_only_elapsed_sessions() {
        let window = Duration::from_millis(50);
        let mut buf = DebounceBuffer::new(window);

        buf.push("fast", "go".to_string());

        thread::sleep(Duration::from_millis(60));

        // Push a second session AFTER sleep so its window has not elapsed.
        buf.push("slow", "wait".to_string());

        let mut ready = buf.flush_all_ready();
        assert_eq!(ready.len(), 1);
        ready.sort_by_key(|(k, _)| k.clone());
        assert_eq!(ready[0].0, "fast");
        assert_eq!(ready[0].1, vec!["go"]);

        // "slow" session should still be buffering.
        assert_eq!(buf.active_sessions(), 1);
    }

    #[test]
    fn flush_all_ready_empty_when_nothing_ready() {
        let mut buf = DebounceBuffer::new(Duration::from_secs(60));
        buf.push("s1", "a".to_string());
        buf.push("s2", "b".to_string());

        let ready = buf.flush_all_ready();
        assert!(ready.is_empty());
        assert_eq!(buf.active_sessions(), 2);
    }

    #[test]
    fn flush_nonexistent_session_returns_none() {
        let mut buf = DebounceBuffer::with_default_window();
        assert!(buf.flush("no-such-session").is_none());
    }
}
