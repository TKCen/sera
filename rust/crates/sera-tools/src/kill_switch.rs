//! Kill switch — CON-04 compliance point for emergency agent halt via Unix socket.

/// Commands that can be sent via the kill switch socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillSwitchCommand {
    Rollback,
}

/// Errors from tool-level kill switch socket operations (bind / IO).
///
/// This is distinct from `sera_gateway::kill_switch::ToolKillError`, which
/// models the gateway-level emergency-stop state machine. `ToolKillError`
/// covers only the lower-level socket bind/IO failures used by the CON-04
/// boot health check and the `listen` helper in this crate.
#[derive(Debug, thiserror::Error)]
pub enum ToolKillError {
    #[error("failed to bind socket: {reason}")]
    BindFailed { reason: String },
    #[error("io error: {reason}")]
    IoError { reason: String },
}

/// CON-04 compliance boot health check.
///
/// Verifies the socket path is valid and the runtime can bind to it.
/// On Unix systems, attempts to bind a `UnixListener` to the path.
/// On non-Unix systems, stubs out (returns Ok).
pub fn boot_health_check(socket_path: &str) -> Result<(), ToolKillError> {
    if socket_path.is_empty() {
        return Err(ToolKillError::BindFailed {
            reason: "socket path is empty".to_string(),
        });
    }

    #[cfg(unix)]
    {
        use std::path::Path;
        let path = Path::new(socket_path);

        // Check parent directory exists and is accessible
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            return Err(ToolKillError::BindFailed {
                reason: format!("parent directory does not exist: {}", parent.display()),
            });
        }

        // Attempt to bind to validate the path is usable.
        // Remove stale socket file if present.
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| ToolKillError::BindFailed {
                reason: format!("could not remove stale socket: {e}"),
            })?;
        }

        // Use std::os::unix::net::UnixListener for the sync boot check
        match std::os::unix::net::UnixListener::bind(path) {
            Ok(listener) => {
                // Successfully bound — drop listener to release the socket
                drop(listener);
                // Clean up the test socket
                let _ = std::fs::remove_file(path);
                Ok(())
            }
            Err(e) => Err(ToolKillError::BindFailed {
                reason: e.to_string(),
            }),
        }
    }

    #[cfg(not(unix))]
    {
        // Non-Unix stub — validate path looks reasonable
        Ok(())
    }
}

/// Listen on a Unix socket for kill switch commands (async, Unix-only).
#[cfg(unix)]
pub async fn listen(socket_path: &str) -> Result<KillSwitchCommand, ToolKillError> {
    use tokio::net::UnixListener;

    let listener = UnixListener::bind(socket_path).map_err(|e| ToolKillError::BindFailed {
        reason: e.to_string(),
    })?;

    let (_stream, _addr) = listener.accept().await.map_err(|e| ToolKillError::IoError {
        reason: e.to_string(),
    })?;

    Ok(KillSwitchCommand::Rollback)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- boot_health_check: failure cases ---

    #[test]
    fn boot_health_check_rejects_empty_path() {
        let err = boot_health_check("").unwrap_err();
        assert!(
            matches!(err, ToolKillError::BindFailed { .. }),
            "expected BindFailed, got {err}"
        );
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn boot_health_check_rejects_nonexistent_parent() {
        let result = boot_health_check("/nonexistent_dir_xyzzy/socket.sock");
        assert!(result.is_err(), "nonexistent parent should fail");
    }

    // --- boot_health_check: success case ---

    #[test]
    #[cfg(unix)]
    fn boot_health_check_succeeds_with_valid_tempdir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("test.sock");
        let path_str = socket_path.to_str().unwrap();
        // Should succeed: parent exists and path is writable
        assert!(
            boot_health_check(path_str).is_ok(),
            "valid tempdir socket path should succeed"
        );
        // Socket file must be cleaned up after the check
        assert!(
            !socket_path.exists(),
            "boot_health_check must remove the test socket file"
        );
    }

    #[test]
    #[cfg(unix)]
    fn boot_health_check_removes_stale_socket() {
        // Pre-create a socket file to simulate a stale leftover
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("stale.sock");
        // Create a regular file at that path to simulate a stale socket
        std::fs::write(&socket_path, b"stale").expect("write stale file");
        assert!(socket_path.exists());

        let path_str = socket_path.to_str().unwrap();
        // boot_health_check must remove the stale file and succeed
        assert!(
            boot_health_check(path_str).is_ok(),
            "should remove stale socket and succeed"
        );
        assert!(
            !socket_path.exists(),
            "stale socket must be cleaned up after check"
        );
    }

    // --- KillSwitchCommand ---

    #[test]
    fn kill_switch_command_clone_and_eq() {
        let cmd = KillSwitchCommand::Rollback;
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    // --- Error display ---

    #[test]
    fn kill_switch_error_bind_failed_display() {
        let err = ToolKillError::BindFailed {
            reason: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("permission denied"));
    }

    #[test]
    fn kill_switch_error_io_error_display() {
        let err = ToolKillError::IoError {
            reason: "broken pipe".to_string(),
        };
        assert!(err.to_string().contains("broken pipe"));
    }
}
