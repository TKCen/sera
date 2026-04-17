//! Capability-token signing and verification for `/api/evolve/*` routes.
//!
//! The evolve pipeline requires a [`sera_types::evolution::CapabilityToken`] on
//! every [`sera_meta::ChangeProposer`] so the policy engine can check whether
//! the proposer holds the [`BlastRadius`] scope they are attempting to act on.
//! Before this module existed, routes synthesised matching-scope tokens with
//! an all-zero signature so the pipeline would accept any request — a known
//! gap flagged in `docs/plan/*` as the signature-verification follow-up to
//! sera-rtu0.
//!
//! This module closes that gap with an **HMAC-SHA-512** construction whose
//! output is exactly 64 bytes, matching the `signature: [u8; 64]` field on
//! [`sera_types::evolution::CapabilityToken`] with no truncation or padding.
//! Verification is constant-time and rejects tokens whose canonical bytes do
//! not match the recomputed MAC, and whose `expires_at` has passed.
//!
//! # Design choice — Path C' (gateway-local verifier, sera-auth untouched)
//!
//! The [`sera_auth::CapabilityToken`] type is a *narrowing* token modelled on
//! [`sera_types::evolution::AgentCapability`] — it has no `signature` field.
//! The evolve pipeline uses a different token shape, from
//! [`sera_types::evolution::CapabilityToken`], whose `signature: [u8; 64]` has
//! no verifier anywhere in the workspace. Modifying `sera-auth`'s public API
//! to add one is out of scope for this increment; instead we add a gateway-
//! local signer/verifier so the gateway has a real signature gate for
//! unsigned and tampered tokens while leaving room for a future sera-auth
//! integration without a second rewrite of the route layer.
//!
//! # Canonical bytes
//!
//! The MAC is computed over:
//!
//! ```text
//! id_len (u32, LE) | id (UTF-8)
//!   | scope_count (u32, LE)
//!   | sorted scope indices (u16 LE each — the discriminant index of the
//!                            BlastRadius variant in declaration order)
//!   | expires_at (i64, LE, unix-millis)
//!   | max_proposals (u32, LE)
//! ```
//!
//! Sorting the scope indices makes the canonical form insensitive to
//! `HashSet<BlastRadius>` iteration order. Encoding lengths explicitly keeps
//! the bytes unambiguous even if a future `BlastRadius` variant is added.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use sera_types::evolution::{BlastRadius, CapabilityToken};
use sha2::{Digest, Sha512};

pub use sera_db::proposal_usage::{
    InMemoryProposalUsageStore, PostgresProposalUsageStore, ProposalUsageStore,
};

/// Errors surfaced by evolve-token verification. Callers map these to HTTP
/// status codes in the route layer (401 for signature/expiry failures, 403
/// for scope mismatches).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EvolveTokenError {
    /// The signature did not match the MAC recomputed over the canonical
    /// bytes. Treat as 401.
    #[error("invalid signature")]
    InvalidSignature,
    /// The token's `expires_at` is in the past. Treat as 401.
    #[error("token expired")]
    Expired,
    /// The token lacks the scope required for this request. Treat as 403.
    #[error("missing required scope: {0:?}")]
    MissingScope(BlastRadius),
    /// The signer secret is empty — configuration error. Treat as 500.
    #[error("signer secret is empty")]
    EmptySecret,
}

/// HMAC-SHA-512 signer for [`CapabilityToken`]s.
///
/// Tokens are signed against a shared secret held by the gateway. In
/// production this secret should be distinct from the JWT and API-key
/// secrets so compromise of one auth surface does not enable minting evolve
/// tokens. During dev, operators can re-use `SERA_JWT_SECRET` if
/// `SERA_EVOLVE_TOKEN_SECRET` is unset.
#[derive(Clone)]
pub struct EvolveTokenSigner {
    secret: Vec<u8>,
}

impl EvolveTokenSigner {
    /// Create a new signer from a raw secret. An empty secret produces a
    /// signer that always fails verification with [`EvolveTokenError::EmptySecret`].
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self { secret: secret.into() }
    }

    /// Compute the canonical bytes for a token (everything except the
    /// signature field itself).
    fn canonical_bytes(token: &CapabilityToken) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + token.id.len());

        // id
        let id_bytes = token.id.as_bytes();
        out.extend_from_slice(&(id_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(id_bytes);

        // scopes — sort the discriminant indices to make the HashSet
        // canonical. We build the discriminant via its serde-rendered
        // position; since `BlastRadius` is `#[non_exhaustive]` and derives
        // `Serialize` in snake_case, we use the `Debug` form as the stable
        // key. That is adequate for signing: both signer and verifier use
        // the same Rust binary, so the exact byte layout only needs to be
        // reproducible across a single process.
        let mut scopes: Vec<String> = token
            .scopes
            .iter()
            .map(|s| format!("{s:?}"))
            .collect();
        scopes.sort();
        out.extend_from_slice(&(scopes.len() as u32).to_le_bytes());
        for s in &scopes {
            let b = s.as_bytes();
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        }

        // expires_at (unix-millis, signed)
        out.extend_from_slice(&token.expires_at.timestamp_millis().to_le_bytes());

        // max_proposals
        out.extend_from_slice(&token.max_proposals.to_le_bytes());

        out
    }

    /// Compute the HMAC-SHA-512 of the canonical bytes under the signer's
    /// secret. Returns a 64-byte array that can be dropped straight into
    /// [`CapabilityToken::signature`].
    ///
    /// Implements HMAC per RFC 2104 with SHA-512's 128-byte block size.
    pub fn mac(&self, token: &CapabilityToken) -> [u8; 64] {
        let canon = Self::canonical_bytes(token);
        hmac_sha512(&self.secret, &canon)
    }

    /// Mint a new signature for `token` and install it. Any previous
    /// signature is overwritten.
    pub fn sign(&self, token: &mut CapabilityToken) {
        token.signature = self.mac(token);
    }

    /// Verify a token's signature, expiry, and that `required` is one of its
    /// scopes. Returns `Ok(())` only when all three checks pass.
    ///
    /// Order of checks matters for the status-code contract:
    /// 1. Empty-secret / signature mismatch → [`EvolveTokenError::InvalidSignature`]
    ///    (401). An attacker must not learn whether a token's scopes include
    ///    the required one unless the signature is legitimate.
    /// 2. Expiry → [`EvolveTokenError::Expired`] (401).
    /// 3. Scope membership → [`EvolveTokenError::MissingScope`] (403).
    pub fn verify(
        &self,
        token: &CapabilityToken,
        required: BlastRadius,
    ) -> Result<(), EvolveTokenError> {
        if self.secret.is_empty() {
            return Err(EvolveTokenError::EmptySecret);
        }

        let expected = self.mac(token);
        if !constant_time_eq_64(&expected, &token.signature) {
            return Err(EvolveTokenError::InvalidSignature);
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if token.expires_at.timestamp_millis() <= now_ms {
            return Err(EvolveTokenError::Expired);
        }

        if !token.scopes.contains(&required) {
            return Err(EvolveTokenError::MissingScope(required));
        }

        Ok(())
    }
}

// ── Proposal-usage tracker ─────────────────────────────────────────────────

/// Error returned when a token has exhausted its `max_proposals` budget.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error(
    "proposal limit reached for token '{token_id}': max_proposals={limit}"
)]
pub struct ProposalLimitError {
    pub token_id: String,
    pub limit: u32,
}

/// In-memory counter tracking how many proposals each token id has used.
///
/// Keyed on [`CapabilityToken::id`]; value is the number of proposals already
/// consumed. The counter is intentionally not persisted — gateway restarts
/// reset counts. A persistence follow-up bead should add a DB-backed backend
/// when durable enforcement is required.
///
/// `std::sync::Mutex` is used because all critical sections are short (HashMap
/// read/write, no `.await`), making `tokio::sync::Mutex` unnecessary overhead.
#[derive(Debug, Default)]
pub struct ProposalUsageTracker {
    counts: Mutex<HashMap<String, u32>>,
}

impl ProposalUsageTracker {
    /// Create a new, empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wrap `Self::new()` in an [`Arc`] — convenience for call sites.
    pub fn new_arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Atomically check whether the token has budget remaining and, if so,
    /// increment the counter.
    ///
    /// Returns `Ok(())` when `used < max_proposals` and the counter has been
    /// bumped. Returns [`ProposalLimitError`] when `used >= max_proposals`;
    /// the counter is **not** incremented in that case.
    pub fn check_and_record(
        &self,
        token: &CapabilityToken,
    ) -> Result<(), ProposalLimitError> {
        let mut counts = self.counts.lock().expect("proposal_usage mutex poisoned");
        let used = counts.entry(token.id.clone()).or_insert(0);
        if *used >= token.max_proposals {
            return Err(ProposalLimitError {
                token_id: token.id.clone(),
                limit: token.max_proposals,
            });
        }
        *used += 1;
        Ok(())
    }

    /// Reset the counter for a specific token id.
    ///
    /// Primarily for test isolation; callers can also use this to clear a
    /// token that has been reissued with a fresh budget.
    pub fn reset(&self, token_id: &str) {
        let mut counts = self.counts.lock().expect("proposal_usage mutex poisoned");
        counts.remove(token_id);
    }
}

/// Constant-time comparison for two 64-byte arrays. Avoids a timing side
/// channel on signature verification.
fn constant_time_eq_64(a: &[u8; 64], b: &[u8; 64]) -> bool {
    let mut acc: u8 = 0;
    for i in 0..64 {
        acc |= a[i] ^ b[i];
    }
    acc == 0
}

/// Minimal HMAC-SHA-512 (RFC 2104). Pulled inline so we don't take on a new
/// dependency for a handful of bytes of glue; `sha2` is already in the
/// workspace.
fn hmac_sha512(key: &[u8], msg: &[u8]) -> [u8; 64] {
    const BLOCK_SIZE: usize = 128; // SHA-512 block size in bytes

    // Key normalisation: if longer than the block size, hash it first.
    let mut k_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let mut h = Sha512::new();
        h.update(key);
        let digest = h.finalize();
        k_block[..digest.len()].copy_from_slice(&digest);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0u8; BLOCK_SIZE];
    let mut opad = [0u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] = k_block[i] ^ 0x36;
        opad[i] = k_block[i] ^ 0x5c;
    }

    // inner = H(ipad || msg)
    let mut inner = Sha512::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();

    // outer = H(opad || inner)
    let mut outer = Sha512::new();
    outer.update(opad);
    outer.update(inner_digest);
    let outer_digest = outer.finalize();

    let mut out = [0u8; 64];
    out.copy_from_slice(&outer_digest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn test_token(scopes: &[BlastRadius], expires_offset_secs: i64) -> CapabilityToken {
        CapabilityToken {
            id: "tok-test".to_string(),
            scopes: scopes.iter().copied().collect::<HashSet<_>>(),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(expires_offset_secs),
            max_proposals: 10,
            signature: [0u8; 64],
        }
    }

    #[test]
    fn sign_then_verify_roundtrip() {
        let signer = EvolveTokenSigner::new(b"secret-key".to_vec());
        let mut tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
        signer.sign(&mut tok);
        assert_eq!(signer.verify(&tok, BlastRadius::SingleHookConfig), Ok(()));
    }

    #[test]
    fn unsigned_token_rejected() {
        let signer = EvolveTokenSigner::new(b"secret-key".to_vec());
        // Default [0u8; 64] signature — should fail under any real secret.
        let tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
        assert_eq!(
            signer.verify(&tok, BlastRadius::SingleHookConfig),
            Err(EvolveTokenError::InvalidSignature)
        );
    }

    #[test]
    fn wrong_scope_rejected_with_missing_scope() {
        let signer = EvolveTokenSigner::new(b"secret-key".to_vec());
        let mut tok = test_token(&[BlastRadius::AgentMemory], 3600);
        signer.sign(&mut tok);
        // Request a different scope than the token was issued for.
        assert_eq!(
            signer.verify(&tok, BlastRadius::GatewayCore),
            Err(EvolveTokenError::MissingScope(BlastRadius::GatewayCore))
        );
    }

    #[test]
    fn expired_token_rejected() {
        let signer = EvolveTokenSigner::new(b"secret-key".to_vec());
        let mut tok = test_token(&[BlastRadius::SingleHookConfig], -3600);
        signer.sign(&mut tok);
        assert_eq!(
            signer.verify(&tok, BlastRadius::SingleHookConfig),
            Err(EvolveTokenError::Expired)
        );
    }

    #[test]
    fn tampered_signature_rejected() {
        let signer = EvolveTokenSigner::new(b"secret-key".to_vec());
        let mut tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
        signer.sign(&mut tok);
        // Flip one bit in the signature.
        tok.signature[0] ^= 0x80;
        assert_eq!(
            signer.verify(&tok, BlastRadius::SingleHookConfig),
            Err(EvolveTokenError::InvalidSignature)
        );
    }

    #[test]
    fn tampered_payload_rejected() {
        let signer = EvolveTokenSigner::new(b"secret-key".to_vec());
        let mut tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
        signer.sign(&mut tok);
        // Change a non-signature field → signature no longer matches.
        tok.max_proposals += 1;
        assert_eq!(
            signer.verify(&tok, BlastRadius::SingleHookConfig),
            Err(EvolveTokenError::InvalidSignature)
        );
    }

    #[test]
    fn different_secret_rejected() {
        let signer_a = EvolveTokenSigner::new(b"secret-a".to_vec());
        let signer_b = EvolveTokenSigner::new(b"secret-b".to_vec());
        let mut tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
        signer_a.sign(&mut tok);
        assert_eq!(
            signer_b.verify(&tok, BlastRadius::SingleHookConfig),
            Err(EvolveTokenError::InvalidSignature)
        );
    }

    #[test]
    fn empty_secret_rejected() {
        let signer = EvolveTokenSigner::new(Vec::<u8>::new());
        let tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
        assert_eq!(
            signer.verify(&tok, BlastRadius::SingleHookConfig),
            Err(EvolveTokenError::EmptySecret)
        );
    }

    #[test]
    fn scope_order_independence() {
        // Two tokens with the same scopes inserted in different orders must
        // produce the same MAC — otherwise HashSet iteration would poison
        // signing.
        let signer = EvolveTokenSigner::new(b"k".to_vec());
        let mut a = test_token(&[BlastRadius::AgentMemory, BlastRadius::GatewayCore], 3600);
        let mut b = test_token(&[BlastRadius::GatewayCore, BlastRadius::AgentMemory], 3600);
        // Force both to have identical id/expires/max so only scope order differs.
        b.expires_at = a.expires_at;
        b.max_proposals = a.max_proposals;
        b.id = a.id.clone();
        assert_eq!(signer.mac(&a), signer.mac(&b));
        signer.sign(&mut a);
        signer.sign(&mut b);
        assert_eq!(a.signature, b.signature);
    }

    #[test]
    fn hmac_sha512_long_key_is_hashed() {
        // Keys longer than the block size (128 bytes for SHA-512) are
        // pre-hashed per RFC 2104. This test ensures our implementation does
        // that rather than truncating — otherwise two different long keys
        // with identical first-128-bytes would produce the same MAC.
        let long_a: Vec<u8> = (0..200).map(|i| i as u8).collect();
        let mut long_b = long_a.clone();
        long_b[150] ^= 0xff; // differ past byte 128
        assert_ne!(hmac_sha512(&long_a, b"msg"), hmac_sha512(&long_b, b"msg"));
    }

    // ── ProposalUsageTracker tests ────────────────────────────────────────

    fn tracker_token(id: &str, max_proposals: u32) -> CapabilityToken {
        CapabilityToken {
            id: id.to_string(),
            scopes: HashSet::new(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals,
            signature: [0u8; 64],
        }
    }

    #[test]
    fn tracker_first_use_accepted_and_counter_increments() {
        // max_proposals=2, first call → Ok (used becomes 1)
        let tracker = ProposalUsageTracker::new();
        let tok = tracker_token("tok-a", 2);
        assert!(tracker.check_and_record(&tok).is_ok());
    }

    #[test]
    fn tracker_second_use_accepted() {
        // max_proposals=2, two calls → both Ok (used becomes 2)
        let tracker = ProposalUsageTracker::new();
        let tok = tracker_token("tok-b", 2);
        assert!(tracker.check_and_record(&tok).is_ok());
        assert!(tracker.check_and_record(&tok).is_ok());
    }

    #[test]
    fn tracker_third_use_rejected_with_limit_error() {
        // max_proposals=2, third call → ProposalLimitError
        let tracker = ProposalUsageTracker::new();
        let tok = tracker_token("tok-c", 2);
        tracker.check_and_record(&tok).expect("first ok");
        tracker.check_and_record(&tok).expect("second ok");
        let err = tracker.check_and_record(&tok).expect_err("third must fail");
        assert_eq!(err.token_id, "tok-c");
        assert_eq!(err.limit, 2);
    }

    #[test]
    fn tracker_different_token_ids_have_independent_counters() {
        // Tokens with different ids must not share a counter.
        let tracker = ProposalUsageTracker::new();
        let tok_x = tracker_token("tok-x", 1);
        let tok_y = tracker_token("tok-y", 1);
        // Exhaust tok-x
        tracker.check_and_record(&tok_x).expect("tok-x first ok");
        tracker.check_and_record(&tok_x).expect_err("tok-x second fails");
        // tok-y is still fresh
        assert!(tracker.check_and_record(&tok_y).is_ok(), "tok-y should succeed");
    }

    #[test]
    fn tracker_reset_clears_counter() {
        // After reset, a previously-exhausted token is accepted again.
        let tracker = ProposalUsageTracker::new();
        let tok = tracker_token("tok-r", 1);
        tracker.check_and_record(&tok).expect("first ok");
        tracker.check_and_record(&tok).expect_err("should be exhausted");
        tracker.reset("tok-r");
        assert!(tracker.check_and_record(&tok).is_ok(), "after reset should succeed");
    }
}
