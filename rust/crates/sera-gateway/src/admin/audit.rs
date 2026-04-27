//! JSONL audit log for the admin HTTP server (sera-nrn9).
//!
//! Every admin request — success or failure — appends one JSON object per line
//! to `<data_root>/admin-audit.jsonl`. The token is never written verbatim:
//! callers pass an opaque [`TokenFingerprint`] that exposes only the first
//! eight hex chars of `sha256(token)`.
//!
//! The writer is intentionally small and synchronous on the file IO side —
//! `tokio::fs::OpenOptions::append` is used inside an `async fn` so the
//! middleware does not need to spawn a blocking task. Failures are logged via
//! `tracing` but never propagated to the request path; an audit-write failure
//! must not turn a 200 response into a 500.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// Opaque first-8-hex-chars-of-sha256(token) wrapper. Logged in place of the
/// raw admin token.
#[derive(Debug, Clone)]
pub struct TokenFingerprint(String);

impl TokenFingerprint {
    pub fn of(token: &str) -> Self {
        let digest = Sha256::digest(token.as_bytes());
        let hex = hex::encode(digest);
        Self(hex[..8].to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One audit row.
#[derive(Debug, Serialize)]
pub struct AdminAuditEntry<'a> {
    pub ts: String,
    pub caller: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub status: u16,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub args: serde_json::Value,
    pub ms: u64,
}

/// Append-only JSONL writer guarded by a tokio Mutex so concurrent admin
/// requests do not interleave their bytes.
pub struct AdminAuditLogger {
    path: PathBuf,
    lock: Mutex<()>,
}

impl AdminAuditLogger {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn shared(path: PathBuf) -> Arc<Self> {
        Arc::new(Self::new(path))
    }

    /// Resolve the audit log path: `<data_root>/admin-audit.jsonl`.
    pub fn default_path<P: AsRef<Path>>(data_root: P) -> PathBuf {
        data_root.as_ref().join("admin-audit.jsonl")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one row. Errors are logged via `tracing::warn!` and swallowed —
    /// the audit log must never break a request.
    pub async fn append(&self, entry: AdminAuditEntry<'_>) {
        let line = match serde_json::to_string(&entry) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "admin audit serialize failed");
                return;
            }
        };

        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            tracing::warn!(error = %e, path = %parent.display(), "admin audit mkdir failed");
            return;
        }

        let _guard = self.lock.lock().await;
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, path = %self.path.display(), "admin audit open failed");
                return;
            }
        };
        if let Err(e) = file.write_all(line.as_bytes()).await {
            tracing::warn!(error = %e, "admin audit write failed");
            return;
        }
        if let Err(e) = file.write_all(b"\n").await {
            tracing::warn!(error = %e, "admin audit newline write failed");
            return;
        }
        if let Err(e) = file.flush().await {
            tracing::warn!(error = %e, "admin audit flush failed");
        }
    }
}

/// Build the canonical timestamp string used in audit rows.
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fingerprint_is_first_eight_hex_of_sha256() {
        let fp = TokenFingerprint::of("hello");
        // sha256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(fp.as_str(), "2cf24dba");
    }

    #[test]
    fn fingerprint_does_not_leak_token() {
        let fp = TokenFingerprint::of("super-secret-admin-token");
        assert_eq!(fp.as_str().len(), 8);
        assert!(!fp.as_str().contains("super"));
        assert!(!fp.as_str().contains("admin"));
    }

    #[tokio::test]
    async fn append_creates_file_and_writes_line() {
        let dir = TempDir::new().unwrap();
        let path = AdminAuditLogger::default_path(dir.path());
        let logger = AdminAuditLogger::new(path.clone());

        logger
            .append(AdminAuditEntry {
                ts: "2026-04-27T07:30:00.000Z".to_string(),
                caller: "abcd1234",
                method: "POST",
                path: "/admin/policies/reload",
                status: 200,
                args: serde_json::Value::Null,
                ms: 12,
            })
            .await;

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.ends_with('\n'));
        let parsed: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(parsed["caller"], "abcd1234");
        assert_eq!(parsed["status"], 200);
        assert_eq!(parsed["path"], "/admin/policies/reload");
    }

    #[tokio::test]
    async fn append_handles_concurrent_writes_without_truncation() {
        let dir = TempDir::new().unwrap();
        let path = AdminAuditLogger::default_path(dir.path());
        let logger = Arc::new(AdminAuditLogger::new(path.clone()));

        let mut handles = Vec::new();
        for i in 0..16 {
            let logger = Arc::clone(&logger);
            handles.push(tokio::spawn(async move {
                logger
                    .append(AdminAuditEntry {
                        ts: now_iso(),
                        caller: "fffffff",
                        method: "GET",
                        path: "/admin/sessions",
                        status: 200,
                        args: serde_json::json!({"i": i}),
                        ms: 1,
                    })
                    .await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 16);
        for line in lines {
            // Every line must parse as JSON — no torn writes.
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
    }
}
