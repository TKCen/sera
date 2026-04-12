//! Inbound deduplication cache — SPEC-gateway §4.1.
//!
//! Real channels redeliver messages after reconnects. This cache maintains a
//! short-lived idempotency key store keyed by `(channel, account, peer,
//! session_key, message_id)`. Duplicate deliveries are detected and silently
//! dropped before entering the routing pipeline.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default TTL for dedupe cache entries (5 minutes per spec).
pub const DEFAULT_DEDUPE_TTL: Duration = Duration::from_secs(300);

/// Composite deduplication key per SPEC-gateway §4.1.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DedupeKey {
    /// The channel the message arrived on (e.g., "discord", "telegram").
    pub channel: String,
    /// The bot account identity on that channel, if the agent has multiple.
    pub account: Option<String>,
    /// The peer (sender) identity.
    pub peer: String,
    /// The session key this message maps to.
    pub session_key: String,
    /// The platform-assigned message ID.
    pub message_id: String,
}

impl DedupeKey {
    /// Serialise the key into a stable cache string.
    ///
    /// Fields are joined with `|` — a character not valid in any component
    /// according to SERA naming conventions.
    pub fn to_key_string(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.channel,
            self.account.as_deref().unwrap_or(""),
            self.peer,
            self.session_key,
            self.message_id,
        )
    }
}

/// Short-lived cache that detects duplicate inbound message deliveries.
///
/// Entries expire after a configurable TTL. Call [`DedupeCache::cleanup`]
/// periodically (e.g., on every inbound batch) to reclaim memory.
#[derive(Debug)]
pub struct DedupeCache {
    ttl: Duration,
    entries: HashMap<String, Instant>,
}

impl DedupeCache {
    /// Create a new cache with the given TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: HashMap::new(),
        }
    }

    /// Create a cache with the default 5-minute TTL.
    pub fn with_default_ttl() -> Self {
        Self::new(DEFAULT_DEDUPE_TTL)
    }

    /// Check whether `key` is a **new** message and record it.
    ///
    /// Returns `true` if the key has **not** been seen before (or has
    /// expired), meaning the message should be processed.
    /// Returns `false` if the key is a duplicate and should be dropped.
    pub fn check_and_insert(&mut self, key: &str) -> bool {
        let now = Instant::now();

        if let Some(inserted_at) = self.entries.get(key)
            && now.duration_since(*inserted_at) < self.ttl
        {
            // Still within TTL — this is a duplicate.
            return false;
        }
        // Expired or not seen — treat as new, fall through to re-insert.

        self.entries.insert(key.to_string(), now);
        true
    }

    /// Remove all entries whose TTL has elapsed.
    ///
    /// Call this regularly to bound memory usage. O(n) over current entries.
    pub fn cleanup(&mut self) {
        let now = Instant::now();
        let ttl = self.ttl;
        self.entries
            .retain(|_, inserted_at| now.duration_since(*inserted_at) < ttl);
    }

    /// Return the number of live (not-yet-cleaned) entries in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return `true` if there are no entries in the cache.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn make_key(msg_id: &str) -> DedupeKey {
        DedupeKey {
            channel: "discord".to_string(),
            account: Some("bot-1".to_string()),
            peer: "user-123".to_string(),
            session_key: "agent:sera:main".to_string(),
            message_id: msg_id.to_string(),
        }
    }

    // --- DedupeKey ---

    #[test]
    fn key_string_with_account() {
        let key = make_key("msg-1");
        let s = key.to_key_string();
        assert_eq!(s, "discord|bot-1|user-123|agent:sera:main|msg-1");
    }

    #[test]
    fn key_string_without_account() {
        let key = DedupeKey {
            channel: "telegram".to_string(),
            account: None,
            peer: "peer-x".to_string(),
            session_key: "agent:sera:main".to_string(),
            message_id: "999".to_string(),
        };
        assert_eq!(key.to_key_string(), "telegram||peer-x|agent:sera:main|999");
    }

    // --- DedupeCache: basic duplicate detection ---

    #[test]
    fn first_insert_is_new() {
        let mut cache = DedupeCache::with_default_ttl();
        assert!(cache.check_and_insert("key-1"));
    }

    #[test]
    fn second_insert_is_duplicate() {
        let mut cache = DedupeCache::with_default_ttl();
        assert!(cache.check_and_insert("key-1"));
        assert!(!cache.check_and_insert("key-1"));
    }

    #[test]
    fn different_keys_are_independent() {
        let mut cache = DedupeCache::with_default_ttl();
        assert!(cache.check_and_insert("key-a"));
        assert!(cache.check_and_insert("key-b"));
        assert!(!cache.check_and_insert("key-a"));
        assert!(!cache.check_and_insert("key-b"));
    }

    // --- DedupeCache: TTL expiry ---

    #[test]
    fn expired_entry_is_treated_as_new() {
        let short_ttl = Duration::from_millis(50);
        let mut cache = DedupeCache::new(short_ttl);

        assert!(cache.check_and_insert("key-1"));
        // While still within TTL, it should be a duplicate.
        assert!(!cache.check_and_insert("key-1"));

        thread::sleep(Duration::from_millis(60));

        // After expiry the key is treated as new again.
        assert!(cache.check_and_insert("key-1"));
    }

    // --- DedupeCache: cleanup ---

    #[test]
    fn cleanup_removes_expired_entries() {
        let short_ttl = Duration::from_millis(50);
        let mut cache = DedupeCache::new(short_ttl);

        cache.check_and_insert("key-1");
        cache.check_and_insert("key-2");
        assert_eq!(cache.len(), 2);

        thread::sleep(Duration::from_millis(60));

        cache.cleanup();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn cleanup_preserves_live_entries() {
        let long_ttl = Duration::from_secs(60);
        let mut cache = DedupeCache::new(long_ttl);

        cache.check_and_insert("keep-me");
        assert_eq!(cache.len(), 1);

        cache.cleanup();
        assert_eq!(cache.len(), 1);
    }

    // --- DedupeKey round-trip via cache ---

    #[test]
    fn dedupe_key_used_with_cache() {
        let mut cache = DedupeCache::with_default_ttl();
        let key = make_key("msg-42");
        let s = key.to_key_string();

        assert!(cache.check_and_insert(&s));
        assert!(!cache.check_and_insert(&s));
    }
}
