//! Kill switch — CON-04 compliance point for emergency agent halt via Unix socket.

/// Commands that can be sent via the kill switch socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillSwitchCommand {
    Rollback,
}

/// Errors from kill switch operations.
#[derive(Debug, thiserror::Error)]
pub enum KillSwitchError {
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
pub fn boot_health_check(socket_path: &str) -> Result<(), KillSwitchError> {
    if socket_path.is_empty() {
        return Err(KillSwitchError::BindFailed {
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
            return Err(KillSwitchError::BindFailed {
                reason: format!("parent directory does not exist: {}", parent.display()),
            });
        }

        // Attempt to bind to validate the path is usable.
        // Remove stale socket file if present.
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| KillSwitchError::BindFailed {
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
            Err(e) => Err(KillSwitchError::BindFailed {
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
pub async fn listen(socket_path: &str) -> Result<KillSwitchCommand, KillSwitchError> {
    use tokio::net::UnixListener;

    let listener = UnixListener::bind(socket_path).map_err(|e| KillSwitchError::BindFailed {
        reason: e.to_string(),
    })?;

    let (_stream, _addr) = listener.accept().await.map_err(|e| KillSwitchError::IoError {
        reason: e.to_string(),
    })?;

    Ok(KillSwitchCommand::Rollback)
}
