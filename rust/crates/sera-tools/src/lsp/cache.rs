//! In-process TTL + mtime-keyed symbol cache.
//!
//! Phase 1 scope — see `docs/plan/LSP-TOOLS-DESIGN.md` §6.
//!
//! The cache is keyed on `(project_root, relative_path, server_version,
//! file_mtime)` so that:
//!
//! * A change to the file's mtime invalidates the entry.
//! * A language-server version bump invalidates the entry.
//! * A 5-minute hard TTL (configurable) guards against build tools that do
//!   not touch mtimes.
//!
//! Time is pluggable via a `Clock` trait so tests can tick time without
//! sleeping.

use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use super::tools::SymbolEntry;

/// Default TTL (5 minutes) — matches the design doc's §6.2 hard cap.
pub const DEFAULT_TTL: Duration = Duration::from_secs(5 * 60);

/// Cache key — identifies one file's symbol overview within one project
/// under one language-server version.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CacheKey {
    pub project_root: PathBuf,
    pub relative_path: PathBuf,
    pub server_version: String,
    pub mtime: SystemTime,
}

/// A cached entry — the symbol list plus the moment it was inserted.
#[derive(Debug, Clone)]
pub struct CachedSymbols {
    pub symbols: Vec<SymbolEntry>,
    pub inserted_at: Instant,
}

/// Time source — real wall-clock in production, controllable in tests.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
}

/// Production clock backed by `Instant::now()`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Thread-safe symbol cache.
#[derive(Clone)]
pub struct SymbolCache {
    entries: Arc<DashMap<CacheKey, CachedSymbols>>,
    ttl: Duration,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for SymbolCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SymbolCache")
            .field("entries", &self.entries.len())
            .field("ttl", &self.ttl)
            .finish()
    }
}

impl Default for SymbolCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolCache {
    pub fn new() -> Self {
        Self::with_clock_and_ttl(Arc::new(SystemClock), DEFAULT_TTL)
    }

    pub fn with_clock_and_ttl(clock: Arc<dyn Clock>, ttl: Duration) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            ttl,
            clock,
        }
    }

    /// Fetch an entry if it exists and is still fresh (within TTL).
    pub fn get(&self, key: &CacheKey) -> Option<Vec<SymbolEntry>> {
        let entry = self.entries.get(key)?;
        let age = self.clock.now().saturating_duration_since(entry.inserted_at);
        if age > self.ttl {
            drop(entry);
            self.entries.remove(key);
            return None;
        }
        Some(entry.symbols.clone())
    }

    /// Insert or replace an entry.
    pub fn put(&self, key: CacheKey, symbols: Vec<SymbolEntry>) {
        self.entries.insert(
            key,
            CachedSymbols {
                symbols,
                inserted_at: self.clock.now(),
            },
        );
    }

    /// Evict every entry whose `relative_path` matches the given path, across
    /// all `project_root` / `server_version` / `mtime` combinations.
    pub fn evict_for_path(&self, relative_path: &std::path::Path) {
        self.entries
            .retain(|k, _| k.relative_path.as_path() != relative_path);
    }

    /// Clear every entry.
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Number of live (possibly stale) entries — test helper.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Convenience alias for readability at call sites.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::tools::{ByteRange, SymbolEntry};
    use std::sync::Mutex;

    /// Deterministic clock — callers tick it with `advance`.
    struct TickClock {
        state: Mutex<Instant>,
    }

    impl TickClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(Instant::now()),
            })
        }
        fn advance(&self, by: Duration) {
            let mut g = self.state.lock().unwrap();
            *g += by;
        }
    }

    impl Clock for TickClock {
        fn now(&self) -> Instant {
            *self.state.lock().unwrap()
        }
    }

    fn sample_symbol(name: &str) -> SymbolEntry {
        SymbolEntry {
            name: name.to_string(),
            kind: lsp_types::SymbolKind::STRUCT,
            range: ByteRange { start: 0, end: 10 },
            children: Vec::new(),
        }
    }

    fn sample_key() -> CacheKey {
        CacheKey {
            project_root: PathBuf::from("/tmp/project"),
            relative_path: PathBuf::from("src/lib.rs"),
            server_version: "rust-analyzer 0.0.0".into(),
            mtime: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn put_get_roundtrip() {
        let cache = SymbolCache::new();
        let key = sample_key();
        cache.put(key.clone(), vec![sample_symbol("Foo")]);
        let got = cache.get(&key).expect("cache hit");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "Foo");
    }

    #[test]
    fn ttl_expiry_invalidates_entry() {
        let clock = TickClock::new();
        let cache = SymbolCache::with_clock_and_ttl(clock.clone(), Duration::from_secs(60));
        let key = sample_key();
        cache.put(key.clone(), vec![sample_symbol("Foo")]);

        // Still fresh 30s in.
        clock.advance(Duration::from_secs(30));
        assert!(cache.get(&key).is_some(), "should be fresh at t=30s");

        // Expired 61s in.
        clock.advance(Duration::from_secs(31));
        assert!(cache.get(&key).is_none(), "should be expired at t=61s");
        // get() evicts expired entries on miss.
        assert!(cache.is_empty());
    }

    #[test]
    fn mtime_mismatch_invalidation() {
        let cache = SymbolCache::new();
        let mut key1 = sample_key();
        let key2 = CacheKey {
            mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(42),
            ..key1.clone()
        };
        cache.put(key1.clone(), vec![sample_symbol("Old")]);
        // A lookup with a different mtime must miss — it's a different key.
        assert!(cache.get(&key2).is_none());
        // The original key still hits.
        assert!(cache.get(&key1).is_some());

        // Simulate a write: bump the mtime, evict the old key explicitly.
        key1.mtime = key2.mtime;
        cache.evict_for_path(std::path::Path::new("src/lib.rs"));
        assert!(cache.is_empty());
    }

    #[test]
    fn evict_for_path_removes_all_versions() {
        let cache = SymbolCache::new();
        let a = sample_key();
        let b = CacheKey {
            server_version: "rust-analyzer 0.0.1".into(),
            ..a.clone()
        };
        cache.put(a, vec![sample_symbol("A")]);
        cache.put(b, vec![sample_symbol("B")]);
        assert_eq!(cache.len(), 2);
        cache.evict_for_path(std::path::Path::new("src/lib.rs"));
        assert!(cache.is_empty());
    }

    #[test]
    fn clear_empties_cache() {
        let cache = SymbolCache::new();
        cache.put(sample_key(), vec![sample_symbol("X")]);
        cache.clear();
        assert!(cache.is_empty());
    }
}
