//! Locate workspace binaries from inside an integration test.
//!
//! Cargo only sets `CARGO_BIN_EXE_<name>` for bins belonging to the same
//! crate as the integration test, and `sera-e2e-harness` has no bins of its
//! own — so the harness walks out from the test binary's path to find
//! sibling bins built by a normal `cargo build`/`cargo test --workspace`
//! invocation.  Returns `None` if the binary hasn't been built — callers
//! treat that as a skip condition rather than an error.

use std::path::PathBuf;

/// Walk up the current exe's parent dirs looking for a sibling binary with
/// the given name.  Matches the pattern used by Cargo's own test harness
/// discovery: `$CARGO_TARGET_DIR/<profile>/deps/<test-binary>` with the
/// target bins at `$CARGO_TARGET_DIR/<profile>/<name>`.
pub fn locate_workspace_bin(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut cur = exe.as_path();
    // Walk up at most 4 levels — `deps/` is one, `<profile>/` is another,
    // and some platforms insert a sysroot level above that.
    for _ in 0..4 {
        cur = cur.parent()?;
        let candidate = {
            #[cfg(windows)]
            {
                cur.join(format!("{name}.exe"))
            }
            #[cfg(not(windows))]
            {
                cur.join(name)
            }
        };
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolve the `sera-gateway` binary — returns `None` if missing (caller skips).
pub fn gateway_bin() -> Option<PathBuf> {
    locate_workspace_bin(crate::GATEWAY_BIN_NAME)
}

/// Resolve the `sera-runtime` binary — returns `None` if missing (caller skips).
pub fn runtime_bin() -> Option<PathBuf> {
    locate_workspace_bin(crate::RUNTIME_BIN_NAME)
}

/// Resolve the `sera` CLI binary — returns `None` if missing (caller skips).
pub fn cli_bin() -> Option<PathBuf> {
    locate_workspace_bin("sera")
}
