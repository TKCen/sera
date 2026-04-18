//! Token storage abstraction for `sera auth login/logout`.
//!
//! Three implementations:
//! - [`KeyringTokenStore`] — OS keychain via the `keyring` crate (macOS/Windows/Linux Secret Service).
//! - [`FileTokenStore`] — plaintext `~/.sera/token` with mode `0600` (unix fallback).
//! - [`MockTokenStore`] — in-memory stub for tests.
//!
//! `KeyringTokenStore::new()` returns `Err` when no suitable keychain daemon is available
//! (e.g. WSL without `gnome-keyring`). Callers should fall back to `FileTokenStore`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

/// Keychain service name used as the keyring entry identifier.
const KEYRING_SERVICE: &str = "sera-cli";
/// Keyring user/account name.
const KEYRING_USER: &str = "sera-token";

/// Abstraction over bearer-token persistence so tests can stub the keychain.
pub trait TokenStore: Send + Sync {
    /// Persist `token` to the backing store.
    fn save(&self, token: &str) -> Result<()>;
    /// Retrieve the stored token, if any.
    fn load(&self) -> Result<Option<String>>;
    /// Remove any stored token.
    fn clear(&self) -> Result<()>;
}

// ---------------------------------------------------------------------------
// KeyringTokenStore
// ---------------------------------------------------------------------------

/// Stores the token in the OS keychain (macOS Keychain, Windows Credential
/// Manager, or Linux Secret Service).
pub struct KeyringTokenStore {
    entry: keyring::Entry,
}

impl KeyringTokenStore {
    /// Create a new entry.  Returns `Err` if the keyring crate cannot locate a
    /// suitable credentials store (e.g. WSL without `gnome-keyring`).
    pub fn new() -> Result<Self> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .context("failed to create keyring entry")?;
        Ok(Self { entry })
    }
}

impl TokenStore for KeyringTokenStore {
    fn save(&self, token: &str) -> Result<()> {
        self.entry
            .set_password(token)
            .context("failed to save token to keyring")
    }

    fn load(&self) -> Result<Option<String>> {
        match self.entry.get_password() {
            Ok(t) => Ok(Some(t)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("failed to read token from keyring: {e}")),
        }
    }

    fn clear(&self) -> Result<()> {
        match self.entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // already gone — not an error
            Err(e) => Err(anyhow::anyhow!("failed to delete token from keyring: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// FileTokenStore
// ---------------------------------------------------------------------------

/// Stores the token as plaintext in `~/.sera/token`.
///
/// On unix, the file is created with mode `0600` so only the owning user can
/// read it.  On non-unix platforms the file is written without explicit
/// permission changes (Windows manages this at the directory level).
pub struct FileTokenStore {
    path: PathBuf,
}

impl FileTokenStore {
    /// Create a store that writes to `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Default path: `~/.sera/token`.
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".sera")
            .join("token")
    }
}

impl TokenStore for FileTokenStore {
    fn save(&self, token: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory: {}", parent.display()))?;
        }
        std::fs::write(&self.path, token)
            .with_context(|| format!("failed to write token file: {}", self.path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&self.path, perms).with_context(|| {
                format!(
                    "failed to set permissions on token file: {}",
                    self.path.display()
                )
            })?;
        }

        Ok(())
    }

    fn load(&self) -> Result<Option<String>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let token = std::fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read token file: {}", self.path.display()))?;
        let trimmed = token.trim().to_owned();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed))
        }
    }

    fn clear(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path).with_context(|| {
                format!("failed to remove token file: {}", self.path.display())
            })?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockTokenStore
// ---------------------------------------------------------------------------

/// In-memory token store for unit tests.
#[derive(Debug, Default, Clone)]
pub struct MockTokenStore {
    inner: Arc<Mutex<Option<String>>>,
}

impl MockTokenStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Peek at the stored value without going through the trait.
    pub fn peek(&self) -> Option<String> {
        self.inner.lock().unwrap().clone()
    }
}

impl TokenStore for MockTokenStore {
    fn save(&self, token: &str) -> Result<()> {
        *self.inner.lock().unwrap() = Some(token.to_owned());
        Ok(())
    }

    fn load(&self) -> Result<Option<String>> {
        Ok(self.inner.lock().unwrap().clone())
    }

    fn clear(&self) -> Result<()> {
        *self.inner.lock().unwrap() = None;
        Ok(())
    }
}

/// Build the best available `TokenStore` for the current platform.
///
/// Tries `KeyringTokenStore` first; falls back to `FileTokenStore` when the
/// keychain is unavailable (e.g. headless CI, WSL without a secret-service
/// daemon).
pub fn best_available_store() -> Box<dyn TokenStore> {
    match KeyringTokenStore::new() {
        Ok(ks) => {
            // Probe with a harmless load — if the keyring daemon isn't running
            // on Linux this will fail with `PlatformFailure`.
            match ks.load() {
                Ok(_) => {
                    tracing::debug!("using OS keyring for token storage");
                    return Box::new(ks);
                }
                Err(e) => {
                    tracing::debug!("keyring probe failed ({e}), falling back to file store");
                }
            }
        }
        Err(e) => {
            tracing::debug!("keyring unavailable ({e}), falling back to file store");
        }
    }
    Box::new(FileTokenStore::new(FileTokenStore::default_path()))
}
