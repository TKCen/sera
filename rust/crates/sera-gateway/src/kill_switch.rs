//! Kill switch — admin socket for emergency rollback.
//!
//! Admin socket at `/var/lib/sera/admin.sock` (Unix, default) with OS
//! file-ownership auth. Bypasses the normal auth stack intentionally — this is
//! the emergency stop. The socket path can be overridden with the
//! `SERA_ADMIN_SOCK` environment variable.
//!
//! Accepted text commands (one per connection, newline-terminated):
//! - `ROLLBACK` — arms the kill switch; new HTTP submissions are rejected with
//!   503. Emits a `KILL_SWITCH_ACTIVATED` audit log entry.
//! - `STATUS`   — returns `Armed` or `Disarmed` + newline.
//! - `DISARM`   — disarms the kill switch; normal serving resumes.
//!
//! Source of truth: SPEC-gateway §7a.4 and SPEC-self-evolution §13.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// Default admin socket path (Unix).
pub const DEFAULT_ADMIN_SOCK: &str = "/var/lib/sera/admin.sock";

/// Returns the admin socket path: `SERA_ADMIN_SOCK` env var if set, otherwise
/// [`DEFAULT_ADMIN_SOCK`].
pub fn admin_sock_path() -> String {
    std::env::var("SERA_ADMIN_SOCK").unwrap_or_else(|_| DEFAULT_ADMIN_SOCK.to_string())
}

/// Kill switch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillSwitchState {
    Disarmed,
    Armed,
}

impl std::fmt::Display for KillSwitchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KillSwitchState::Disarmed => write!(f, "Disarmed"),
            KillSwitchState::Armed => write!(f, "Armed"),
        }
    }
}

/// Kill switch controller.
pub struct KillSwitch {
    armed: Arc<AtomicBool>,
}

impl KillSwitch {
    pub fn new() -> Self {
        Self {
            armed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Check if the kill switch is armed.
    pub fn state(&self) -> KillSwitchState {
        if self.armed.load(Ordering::SeqCst) {
            KillSwitchState::Armed
        } else {
            KillSwitchState::Disarmed
        }
    }

    /// Returns `true` if the kill switch is armed (inline hot-path check).
    #[inline]
    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::SeqCst)
    }

    /// Arm the kill switch.
    pub fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Disarm the kill switch.
    pub fn disarm(&self) {
        self.armed.store(false, Ordering::SeqCst);
    }

    /// CON-04 boot-time check — returns error if armed.
    pub fn boot_check(&self) -> Result<(), KillSwitchError> {
        if self.state() == KillSwitchState::Armed {
            Err(KillSwitchError::ArmedAtBoot)
        } else {
            Ok(())
        }
    }

    /// Handle a command received on the admin socket.
    ///
    /// Returns the text response to send back to the client and whether a
    /// `ROLLBACK` was executed (so callers can emit an audit event).
    pub fn handle_command(&self, command: &str) -> (String, bool) {
        match command.trim() {
            "ROLLBACK" => {
                self.arm();
                tracing::warn!(
                    event = "KILL_SWITCH_ACTIVATED",
                    "Admin kill switch armed via ROLLBACK command"
                );
                ("OK\n".to_string(), true)
            }
            "STATUS" => (format!("{}\n", self.state()), false),
            "DISARM" => {
                self.disarm();
                tracing::info!("Admin kill switch disarmed");
                ("OK\n".to_string(), false)
            }
            other => {
                tracing::warn!(command = %other, "Unknown admin socket command");
                ("ERR: unknown command\n".to_string(), false)
            }
        }
    }
}

impl Default for KillSwitch {
    fn default() -> Self {
        Self::new()
    }
}

/// Kill switch errors.
#[derive(Debug, thiserror::Error)]
pub enum KillSwitchError {
    #[error("kill switch armed at boot — refusing to start")]
    ArmedAtBoot,
    #[error("admin socket error: {0}")]
    SocketError(String),
}

/// Spawn the admin Unix socket listener as a background tokio task.
///
/// The task accepts one command per connection (reads until newline/EOF),
/// dispatches via [`KillSwitch::handle_command`], writes the response, and
/// closes. Authentication is by OS file ownership (`0600` permissions).
///
/// `on_rollback` is called (synchronously within the task) after every
/// successful `ROLLBACK` command — use it to emit DB audit events.
///
/// This function is a no-op on non-Unix targets (see `#[cfg(unix)]`).
#[cfg(unix)]
pub fn spawn_admin_socket<F>(ks: Arc<KillSwitch>, socket_path: String, on_rollback: F)
where
    F: Fn() + Send + Sync + 'static,
{
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    tokio::spawn(async move {
        // Remove stale socket file so bind succeeds across restarts.
        let _ = std::fs::remove_file(&socket_path);

        let listener = match UnixListener::bind(&socket_path) {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(
                    path = %socket_path,
                    error = %e,
                    "Failed to bind admin socket"
                );
                return;
            }
        };

        // Restrict to owner-only access.
        if let Err(e) =
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
        {
            tracing::warn!(path = %socket_path, error = %e, "Could not set admin socket permissions to 0600");
        }

        tracing::info!(path = %socket_path, "Admin kill-switch socket listening");

        loop {
            let (mut stream, _addr) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::warn!(error = %e, "Admin socket accept error");
                    continue;
                }
            };

            let ks = Arc::clone(&ks);
            let on_rollback = &on_rollback;

            // Read until newline or EOF (max 256 bytes to avoid abuse).
            let mut buf = vec![0u8; 256];
            let n = match stream.read(&mut buf).await {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(error = %e, "Admin socket read error");
                    continue;
                }
            };
            let command = String::from_utf8_lossy(&buf[..n]);
            let (response, did_rollback) = ks.handle_command(&command);
            if did_rollback {
                on_rollback();
            }
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
}

/// No-op stub for non-Unix targets.
#[cfg(not(unix))]
pub fn spawn_admin_socket<F>(_ks: Arc<KillSwitch>, _socket_path: String, _on_rollback: F)
where
    F: Fn() + Send + Sync + 'static,
{
    tracing::info!("Admin kill-switch socket not supported on this platform");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_disarmed() {
        let ks = KillSwitch::new();
        assert_eq!(ks.state(), KillSwitchState::Disarmed);
        assert!(!ks.is_armed());
    }

    #[test]
    fn rollback_arms() {
        let ks = KillSwitch::new();
        let (resp, did_rollback) = ks.handle_command("ROLLBACK");
        assert_eq!(resp, "OK\n");
        assert!(did_rollback);
        assert_eq!(ks.state(), KillSwitchState::Armed);
        assert!(ks.is_armed());
    }

    #[test]
    fn disarm_after_rollback() {
        let ks = KillSwitch::new();
        ks.handle_command("ROLLBACK");
        let (resp, did_rollback) = ks.handle_command("DISARM");
        assert_eq!(resp, "OK\n");
        assert!(!did_rollback);
        assert_eq!(ks.state(), KillSwitchState::Disarmed);
    }

    #[test]
    fn status_when_disarmed() {
        let ks = KillSwitch::new();
        let (resp, did_rollback) = ks.handle_command("STATUS");
        assert_eq!(resp, "Disarmed\n");
        assert!(!did_rollback);
    }

    #[test]
    fn status_when_armed() {
        let ks = KillSwitch::new();
        ks.arm();
        let (resp, _) = ks.handle_command("STATUS");
        assert_eq!(resp, "Armed\n");
    }

    #[test]
    fn unknown_command() {
        let ks = KillSwitch::new();
        let (resp, did_rollback) = ks.handle_command("NUKE");
        assert!(resp.starts_with("ERR:"));
        assert!(!did_rollback);
    }

    #[test]
    fn rollback_idempotent() {
        let ks = KillSwitch::new();
        ks.handle_command("ROLLBACK");
        let (resp, did_rollback) = ks.handle_command("ROLLBACK");
        assert_eq!(resp, "OK\n");
        assert!(did_rollback);
        assert!(ks.is_armed());
    }

    #[test]
    fn resume_idempotent() {
        let ks = KillSwitch::new();
        let (resp, _) = ks.handle_command("DISARM");
        assert_eq!(resp, "OK\n");
        assert!(!ks.is_armed());
    }

    #[test]
    fn boot_check_passes_when_disarmed() {
        let ks = KillSwitch::new();
        assert!(ks.boot_check().is_ok());
    }

    #[test]
    fn boot_check_fails_when_armed() {
        let ks = KillSwitch::new();
        ks.arm();
        assert!(ks.boot_check().is_err());
    }

    #[test]
    fn trailing_whitespace_trimmed() {
        let ks = KillSwitch::new();
        let (resp, did_rollback) = ks.handle_command("ROLLBACK\n");
        assert_eq!(resp, "OK\n");
        assert!(did_rollback);
    }

    /// Socket path resolves to env var when set.
    #[test]
    fn sock_path_from_env() {
        // SAFETY: test-only; single-threaded test context.
        unsafe { std::env::set_var("SERA_ADMIN_SOCK", "/tmp/test-admin.sock") };
        assert_eq!(admin_sock_path(), "/tmp/test-admin.sock");
        unsafe { std::env::remove_var("SERA_ADMIN_SOCK") };
    }

    /// Socket path falls back to default when env var not set.
    #[test]
    fn sock_path_default() {
        unsafe { std::env::remove_var("SERA_ADMIN_SOCK") };
        // Only check it equals the constant; actual value is platform default.
        assert_eq!(admin_sock_path(), DEFAULT_ADMIN_SOCK);
    }
}

#[cfg(all(test, unix))]
mod socket_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    fn tmp_sock() -> String {
        format!("/tmp/sera-test-admin-{}.sock", std::process::id())
    }

    #[tokio::test]
    async fn socket_rollback_arms_kill_switch() {
        let ks = Arc::new(KillSwitch::new());
        let path = tmp_sock();
        let rollback_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&rollback_called);

        spawn_admin_socket(Arc::clone(&ks), path.clone(), move || {
            flag.store(true, Ordering::SeqCst);
        });

        // Give the listener a moment to bind.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut stream = UnixStream::connect(&path).unwrap();
        stream.write_all(b"ROLLBACK\n").unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert_eq!(resp, "OK\n");

        // Give the task a moment to process.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(ks.is_armed());
        assert!(rollback_called.load(Ordering::SeqCst));

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn socket_status_returns_state() {
        let ks = Arc::new(KillSwitch::new());
        let path = format!("/tmp/sera-test-status-{}.sock", std::process::id());

        spawn_admin_socket(Arc::clone(&ks), path.clone(), || {});
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut stream = UnixStream::connect(&path).unwrap();
        stream.write_all(b"STATUS\n").unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert_eq!(resp, "Disarmed\n");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn socket_disarm_after_rollback() {
        let ks = Arc::new(KillSwitch::new());
        let path = format!("/tmp/sera-test-disarm-{}.sock", std::process::id());

        spawn_admin_socket(Arc::clone(&ks), path.clone(), || {});
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Arm it.
        {
            let mut s = UnixStream::connect(&path).unwrap();
            s.write_all(b"ROLLBACK\n").unwrap();
            let mut r = String::new();
            s.read_to_string(&mut r).unwrap();
            assert_eq!(r, "OK\n");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(ks.is_armed());

        // Disarm it.
        {
            let mut s = UnixStream::connect(&path).unwrap();
            s.write_all(b"DISARM\n").unwrap();
            let mut r = String::new();
            s.read_to_string(&mut r).unwrap();
            assert_eq!(r, "OK\n");
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(!ks.is_armed());

        let _ = std::fs::remove_file(&path);
    }
}
