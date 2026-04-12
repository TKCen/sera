//! Verify that set_audit_backend panics on double-set.

use async_trait::async_trait;
use sera_telemetry::audit::{AuditBackend, AuditEntry, AuditError, set_audit_backend};

struct NoopBackend;

#[async_trait]
impl AuditBackend for NoopBackend {
    async fn append(&self, entry: AuditEntry) -> Result<AuditEntry, AuditError> {
        Ok(entry)
    }
    async fn verify_chain(&self) -> Result<usize, AuditError> {
        Ok(0)
    }
}

static BACKEND_A: NoopBackend = NoopBackend;
static BACKEND_B: NoopBackend = NoopBackend;

#[test]
#[should_panic(expected = "double-set is not permitted")]
fn double_set_panics() {
    set_audit_backend(&BACKEND_A);
    set_audit_backend(&BACKEND_B);
}
