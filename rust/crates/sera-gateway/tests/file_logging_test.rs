//! Integration test — confirms that the file-logging construction logic
//! (rolling appender + non-blocking writer) works without panic when
//! `SERA_LOG_DIR` points to a tempdir.
//!
//! We cannot call `init_file_logging()` directly because it calls `.init()` on
//! the global subscriber (which panics if already set in the same process), so
//! we replicate the core construction here: build a `RollingFileAppender` →
//! wrap in `non_blocking` → verify the guard is returned without panic.

use tempfile::TempDir;
use tracing_appender::non_blocking;
use tracing_appender::rolling;

/// Constructing a rolling daily appender against a real tempdir must not panic.
#[test]
fn file_appender_builds_without_panic() {
    let dir = TempDir::new().expect("tempdir");

    // Simulate what init_file_logging does with SERA_LOG_DIR.
    let log_dir = dir.path().to_str().expect("valid utf-8 path");
    let appender = rolling::daily(log_dir, "sera.log");
    let (_non_blocking_writer, _guard) = non_blocking(appender);

    // If we reach here the construction succeeded.
    assert!(dir.path().exists());
}

/// Hourly rotation variant also builds without panic.
#[test]
fn file_appender_hourly_builds_without_panic() {
    let dir = TempDir::new().expect("tempdir");
    let log_dir = dir.path().to_str().expect("valid utf-8 path");
    let appender = rolling::hourly(log_dir, "sera.log");
    let (_writer, _guard) = non_blocking(appender);
    assert!(dir.path().exists());
}

/// `SERA_LOG_LEVEL` parsing — verify common filter strings don't crash.
#[test]
fn log_level_filter_parses() {
    for level in ["info", "debug", "warn", "error", "trace", "info,sera_gateway=debug"] {
        let filter = tracing_subscriber::EnvFilter::new(level);
        drop(filter);
    }
}
