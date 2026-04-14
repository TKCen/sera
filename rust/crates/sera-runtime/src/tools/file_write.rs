//! Locked file-write helper used by the file-write and file-edit tools.

use std::io::Write as _;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use fs2::FileExt;
use tracing::warn;

// ── Escape-hatch warning (fires once per process) ────────────────────────────

static LOCK_DISABLED_WARNED: OnceLock<()> = OnceLock::new();

fn warn_once_lock_disabled() {
    LOCK_DISABLED_WARNED.get_or_init(|| {
        warn!("SERA_FILE_LOCK_DISABLED=1 — file write lock bypassed");
    });
}

/// Return a random jitter in `0..range_ms` milliseconds.
fn rand_jitter(range_ms: u64) -> Duration {
    use rand::Rng;
    let ms = rand::thread_rng().gen_range(0..range_ms);
    Duration::from_millis(ms)
}

// ── Core sync helper (runs inside spawn_blocking) ────────────────────────────

/// Write `content` to `path` under an exclusive advisory lock.
///
/// The lock covers the full open → truncate → write sequence so concurrent
/// writers never interleave bytes.  Three attempts are made with exponential
/// back-off plus random jitter before returning an error.
///
/// Set `SERA_FILE_LOCK_DISABLED=1` to bypass locking (e.g. in environments
/// where advisory locks are unavailable).  A one-shot warning is emitted to
/// the tracing log the first time this bypass fires.
pub fn locked_write_sync(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    if std::env::var("SERA_FILE_LOCK_DISABLED").as_deref() == Ok("1") {
        warn_once_lock_disabled();
        std::fs::write(path, content)
            .map_err(|e| anyhow::anyhow!("file write error: {e}"))?;
        return Ok(());
    }

    // Resolve the parent so we can canonicalise even when the file does not
    // exist yet (canonicalize fails on missing paths).
    let parent = path.parent().unwrap_or(Path::new("."));
    let canonical_parent = std::fs::canonicalize(parent)
        .map_err(|e| anyhow::anyhow!("cannot resolve parent directory: {e}"))?;
    let canonical = canonical_parent.join(
        path.file_name()
            .ok_or_else(|| anyhow::anyhow!("path has no file name"))?,
    );

    // Open (or create) the file.  We do NOT truncate yet — truncation happens
    // after we hold the lock so no other writer sees a zero-byte window.
    // truncate(false): we truncate manually *after* acquiring the lock so no
    // other writer sees a zero-byte window.
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&canonical)
        .map_err(|e| anyhow::anyhow!("cannot open file for writing: {e}"))?;

    // Retry loop: up to 3 attempts, exponential back-off + jitter.
    let mut delay = Duration::from_millis(50);
    for attempt in 0..3u32 {
        match file.try_lock_exclusive() {
            Ok(()) => {
                // Truncate now that we own the lock.
                file.set_len(0)
                    .map_err(|e| anyhow::anyhow!("truncate failed: {e}"))?;
                (&file)
                    .write_all(content)
                    .map_err(|e| anyhow::anyhow!("write failed: {e}"))?;
                let _ = file.unlock();
                return Ok(());
            }
            Err(_) if attempt < 2 => {
                std::thread::sleep(delay + rand_jitter(150));
                delay *= 2;
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "file locked — could not acquire exclusive lock on {}: {e}",
                    canonical.display()
                ));
            }
        }
    }
    unreachable!()
}

/// Async wrapper: offloads the blocking lock+write to a dedicated thread.
pub async fn locked_write(path: &Path, content: Vec<u8>) -> anyhow::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || locked_write_sync(&path, &content))
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))?
}
