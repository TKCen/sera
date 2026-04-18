//! Advisory file-lock concurrency tests for the file-write helper.

use std::thread;

use sera_runtime::tools::file_write::locked_write_sync;

// ── Test 1: 8 concurrent writers, final content belongs to exactly one ───────

#[test]
fn concurrent_writers_no_interleaving() {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let path = tmp.path().to_path_buf();

    // Pre-create the file so canonicalize works inside the helper.
    std::fs::write(&path, b"").expect("init");

    let handles: Vec<_> = (0..8u32)
        .map(|id| {
            let p = path.clone();
            thread::spawn(move || {
                // Each writer produces a unique, large, easily-identifiable payload.
                let payload = format!("thread-{id}\n").repeat(1024);
                locked_write_sync(&p, payload.as_bytes())
                    .unwrap_or_else(|e| panic!("writer {id} failed: {e}"));
                id
            })
        })
        .collect();

    // Collect which writers succeeded (all should).
    let succeeded: Vec<u32> = handles
        .into_iter()
        .map(|h| h.join().expect("thread panicked"))
        .collect();
    assert_eq!(succeeded.len(), 8, "all 8 writers must complete");

    // Final content must match exactly one writer's full payload — no interleaving.
    let final_content = std::fs::read_to_string(&path).expect("read final");
    let matched = (0..8u32).filter(|id| {
        let expected = format!("thread-{id}\n").repeat(1024);
        final_content == expected
    });
    assert_eq!(
        matched.count(),
        1,
        "final file content must equal exactly one writer's payload (no interleaving)"
    );
}

// ── Test 2: bypass via SERA_FILE_LOCK_DISABLED=1 ─────────────────────────────

#[test]
fn lock_disabled_bypass() {
    // SAFETY: setting env vars in tests is inherently racy when run in parallel;
    // but since we only read the env var in locked_write_sync (not write it from
    // multiple threads simultaneously here), and this test is isolated from the
    // other test via separate process invocation in CI, this is acceptable.
    unsafe { std::env::set_var("SERA_FILE_LOCK_DISABLED", "1") };

    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    let path = tmp.path().to_path_buf();
    std::fs::write(&path, b"").expect("init");

    let handles: Vec<_> = (0..8u32)
        .map(|id| {
            let p = path.clone();
            thread::spawn(move || {
                let payload = format!("bypass-thread-{id}\n").repeat(512);
                // Must NOT return a "file locked" error even under bypass mode.
                locked_write_sync(&p, payload.as_bytes())
                    .unwrap_or_else(|e| panic!("bypass writer {id} failed: {e}"));
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    // Content may be interleaved (bypass = no lock), but must be valid UTF-8
    // and non-empty — at least one writer won.
    let final_content = std::fs::read_to_string(&path).expect("read final");
    assert!(!final_content.is_empty(), "file must not be empty after bypass writes");

    unsafe { std::env::remove_var("SERA_FILE_LOCK_DISABLED") };
}
