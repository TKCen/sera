//! Revocation-aware verification helper (sera-2q6w).
//!
//! [`EvolveTokenSigner::verify`] is sync (the signer holds an `RwLock` and
//! cannot await). The cascade-revocation store is async. [`verify_revocable`]
//! glues the two: it runs the signer's normal sync verify (signature, expiry,
//! scope) and then, on success, consults the [`RevocationStore`] for both the
//! token's `instance_id` and its `parent_id`. If either is recorded as
//! revoked, verification fails with [`EvolveTokenError::Revoked`].
//!
//! Why both `instance_id` and `parent_id`? CapabilityToken (sera-s64j) carries
//! one level of chain visibility on the wire. A revocation against any
//! ancestor is recorded directly via [`RevocationStore::revoke_cascade`] —
//! the cascade walk on the store side propagates the revocation marker
//! through the parent_id graph. The verifier checks the two visible hops the
//! wire token exposes; deeper ancestors that are revoked end up materialized
//! as direct revocation rows when the cascade walker descends past them, so
//! the leaf-side check stays O(2) lookups.

use crate::capability::{CapabilityToken, EvolveTokenError, EvolveTokenSigner};
use sera_db::capability_revocation::RevocationStore;
use sera_types::evolution::BlastRadius;

/// Verify a [`CapabilityToken`] against signature, expiry, scope, AND the
/// cascade-revocation store (sera-2q6w).
///
/// Returns [`EvolveTokenError::Revoked`] when either the token's
/// `instance_id` or its `parent_id` has been recorded as revoked under
/// `tenant_id`. Tokens without `instance_id` (legacy chain-less wire format)
/// are still subject to a `parent_id` check — they just have one fewer
/// lookup.
pub async fn verify_revocable<S: RevocationStore + ?Sized>(
    signer: &EvolveTokenSigner,
    token: &CapabilityToken,
    required: BlastRadius,
    revocations: &S,
    tenant_id: &str,
) -> Result<(), EvolveTokenError> {
    signer.verify(token, required)?;

    if let Some(instance_id) = token.instance_id.as_deref()
        && revocations
            .is_revoked(tenant_id, instance_id)
            .await
            .map_err(|e| EvolveTokenError::RevocationLookupFailed(e.to_string()))?
    {
        return Err(EvolveTokenError::Revoked(instance_id.to_string()));
    }

    if let Some(parent_id) = token.parent_id.as_deref()
        && revocations
            .is_revoked(tenant_id, parent_id)
            .await
            .map_err(|e| EvolveTokenError::RevocationLookupFailed(e.to_string()))?
    {
        return Err(EvolveTokenError::Revoked(format!(
            "ancestor {parent_id} revoked"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{CapabilityToken, DefaultCapabilityTokenIssuer, EvolveTokenSigner};
    use async_trait::async_trait;
    use sera_db::DbError;
    use sera_db::capability_revocation::{InMemoryRevocationStore, RevocationStore};
    use sera_types::evolution::BlastRadius;
    use sera_types::principal::Principal;
    use std::collections::HashSet;

    #[derive(Debug)]
    struct FailingRevocationStore;

    #[async_trait]
    impl RevocationStore for FailingRevocationStore {
        async fn record(
            &self,
            _tenant_id: &str,
            _instance_id: &str,
            _parent_id: Option<&str>,
            _reason: &str,
            _revoked_by: &str,
        ) -> Result<bool, DbError> {
            Err(DbError::Integrity(
                "revocation store unavailable".to_string(),
            ))
        }

        async fn revoke_cascade(
            &self,
            _tenant_id: &str,
            _instance_id: &str,
            _parent_id: Option<&str>,
            _reason: &str,
            _revoked_by: &str,
        ) -> Result<u32, DbError> {
            Err(DbError::Integrity(
                "revocation store unavailable".to_string(),
            ))
        }

        async fn is_revoked(&self, _tenant_id: &str, _instance_id: &str) -> Result<bool, DbError> {
            Err(DbError::Integrity(
                "revocation store unavailable".to_string(),
            ))
        }
    }

    fn signer_for_test() -> EvolveTokenSigner {
        EvolveTokenSigner::new(b"sera-2q6w-test-secret".to_vec())
    }

    fn issue_root() -> CapabilityToken {
        use crate::capability::CapabilityTokenIssuer;
        let issuer = DefaultCapabilityTokenIssuer::new();
        let mut scopes = HashSet::new();
        scopes.insert(BlastRadius::AgentMemory);
        let mut tok = issuer.issue(
            "tok-2q6w-root".to_string(),
            scopes,
            5,
            std::time::Duration::from_secs(3600),
        );
        signer_for_test().sign(&mut tok);
        tok
    }

    fn narrow_signed(parent: &CapabilityToken, by: &Principal) -> CapabilityToken {
        let mut scopes = HashSet::new();
        scopes.insert(BlastRadius::AgentMemory);
        let mut child = parent.narrow(scopes, by, 5).expect("narrow");
        signer_for_test().sign(&mut child);
        child
    }

    #[tokio::test]
    async fn unrevoked_token_passes() {
        let signer = signer_for_test();
        let tok = issue_root();
        let store = InMemoryRevocationStore::new();
        let result =
            verify_revocable(&signer, &tok, BlastRadius::AgentMemory, &store, "tenant-a").await;
        assert!(result.is_ok(), "unrevoked token must verify: {result:?}");
    }

    #[tokio::test]
    async fn directly_revoked_instance_id_is_rejected() {
        let signer = signer_for_test();
        let tok = issue_root();
        let store = InMemoryRevocationStore::new();
        let instance = tok
            .instance_id
            .clone()
            .expect("issued tokens carry instance_id");
        store
            .revoke_cascade("tenant-a", &instance, None, "operator request", "admin")
            .await
            .unwrap();
        let err = verify_revocable(&signer, &tok, BlastRadius::AgentMemory, &store, "tenant-a")
            .await
            .unwrap_err();
        match err {
            EvolveTokenError::Revoked(payload) => {
                assert_eq!(payload, instance);
            }
            other => panic!("expected Revoked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn revoked_parent_rejects_child_verify() {
        let signer = signer_for_test();
        let parent = issue_root();
        let alice = Principal::for_agent("alice", "alice");
        let child = narrow_signed(&parent, &alice);

        let parent_instance = parent.instance_id.clone().unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(parent_instance.as_str()));

        let store = InMemoryRevocationStore::new();
        store
            .revoke_cascade("tenant-a", &parent_instance, None, "compromise", "admin")
            .await
            .unwrap();

        let err = verify_revocable(
            &signer,
            &child,
            BlastRadius::AgentMemory,
            &store,
            "tenant-a",
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, EvolveTokenError::Revoked(ref msg) if msg.contains(&parent_instance)),
            "expected ancestor revocation, got {err:?}"
        );
    }

    #[tokio::test]
    async fn cascade_persistence_rejects_grandchild_by_instance_id() {
        let signer = signer_for_test();
        let root = issue_root();
        let alice = Principal::for_agent("alice", "alice");
        let bob = Principal::for_agent("bob", "bob");
        let child = narrow_signed(&root, &alice);
        let grandchild = narrow_signed(&child, &bob);

        let root_instance = root.instance_id.clone().unwrap();
        let child_instance = child.instance_id.clone().unwrap();
        let grandchild_instance = grandchild.instance_id.clone().unwrap();

        let store = InMemoryRevocationStore::new();
        store
            .record(
                "tenant-a",
                &child_instance,
                Some(&root_instance),
                "chain metadata",
                "issuer",
            )
            .await
            .unwrap();
        store
            .record(
                "tenant-a",
                &grandchild_instance,
                Some(&child_instance),
                "chain metadata",
                "issuer",
            )
            .await
            .unwrap();

        assert!(
            !store
                .is_revoked("tenant-a", &grandchild_instance)
                .await
                .unwrap()
        );

        store
            .revoke_cascade("tenant-a", &root_instance, None, "compromise", "admin")
            .await
            .unwrap();

        let err = verify_revocable(
            &signer,
            &grandchild,
            BlastRadius::AgentMemory,
            &store,
            "tenant-a",
        )
        .await
        .unwrap_err();

        assert_eq!(err, EvolveTokenError::Revoked(grandchild_instance));
    }

    #[tokio::test]
    async fn revocation_in_other_tenant_does_not_reject() {
        let signer = signer_for_test();
        let tok = issue_root();
        let instance = tok.instance_id.clone().unwrap();
        let store = InMemoryRevocationStore::new();
        store
            .revoke_cascade("tenant-other", &instance, None, "admin", "op")
            .await
            .unwrap();
        let result =
            verify_revocable(&signer, &tok, BlastRadius::AgentMemory, &store, "tenant-a").await;
        assert!(result.is_ok(), "cross-tenant revocations must not bleed");
    }

    #[tokio::test]
    async fn revocation_check_runs_after_signature_check() {
        // A bad signature must surface InvalidSignature even if the token's
        // instance_id is also revoked — defense in depth, no error masking.
        let signer = signer_for_test();
        let mut tok = issue_root();
        tok.signature = [0xff; 64]; // tamper
        let store = InMemoryRevocationStore::new();
        let instance = tok.instance_id.clone().unwrap();
        store
            .revoke_cascade("tenant-a", &instance, None, "audit", "op")
            .await
            .unwrap();
        let err = verify_revocable(&signer, &tok, BlastRadius::AgentMemory, &store, "tenant-a")
            .await
            .unwrap_err();
        assert_eq!(err, EvolveTokenError::InvalidSignature);
    }

    #[tokio::test]
    async fn revocation_lookup_failure_is_not_reported_as_revoked() {
        let signer = signer_for_test();
        let tok = issue_root();
        let store = FailingRevocationStore;

        let err = verify_revocable(&signer, &tok, BlastRadius::AgentMemory, &store, "tenant-a")
            .await
            .unwrap_err();

        assert!(
            matches!(err, EvolveTokenError::RevocationLookupFailed(ref msg) if msg.contains("revocation store unavailable")),
            "expected lookup failure, got {err:?}"
        );
    }
}
