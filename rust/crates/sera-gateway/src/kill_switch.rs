//! Kill switch — admin socket for emergency rollback.
//!
//! Admin socket at /var/lib/sera/admin.sock (Unix) with OS file-ownership auth.
//! Bypasses auth stack intentionally — this is the emergency stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Kill switch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillSwitchState {
    Disarmed,
    Armed,
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
    pub fn handle_command(&self, command: &str) -> String {
        match command.trim() {
            "ROLLBACK" => {
                self.arm();
                "OK\n".to_string()
            }
            "STATUS" => {
                format!("{:?}\n", self.state())
            }
            "DISARM" => {
                self.disarm();
                "OK\n".to_string()
            }
            _ => "ERR: unknown command\n".to_string(),
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
