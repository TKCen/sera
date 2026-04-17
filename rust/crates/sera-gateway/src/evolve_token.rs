//! Capability-token signing and verification for `/api/evolve/*` routes.
//!
//! The evolve pipeline requires a [`sera_auth::CapabilityToken`] on
//! every [`sera_auth::ChangeProposer`] so the policy engine can check whether
//! the proposer holds the [`BlastRadius`] scope they are attempting to act on.
//! Before this module existed, routes synthesised matching-scope tokens with
//! an all-zero signature so the pipeline would accept any request — a known
//! gap flagged in `docs/plan/*` as the signature-verification follow-up to
//! sera-rtu0.
//!
//! This module closes that gap with an **HMAC-SHA-512** construction whose
//! output is exactly 64 bytes, matching the `signature: [u8; 64]` field on
//! [`sera_auth::CapabilityToken`] with no truncation or padding.
//! Verification is constant-time and rejects tokens whose canonical bytes do
//! not match the recomputed MAC, and whose `expires_at` has passed.
//!
//! # Signer/issuer split
//!
//! [`CapabilityToken`] values are *issued* by
//! [`sera_auth::CapabilityTokenIssuer`] (constructs the token value from a
//! scope set + expiry policy) and *signed* here by [`EvolveTokenSigner`]
//! (installs the HMAC over the canonical serialisation). Keeping the two
//! orthogonal lets the issuer live in `sera-auth` while the signer's secret
//! handling, rotation, and grace-period logic stay gateway-local.
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
//!
//! # Live key rotation
//!
//! [`EvolveTokenSigner`] now holds its signing key behind an
//! `Arc<RwLock<SigningKey>>` (std, not tokio — critical sections are short,
//! no `.await` inside) and keeps a bounded [`RotationHistory`] (capacity 2)
//! of previous keys with their rotation timestamps.
//!
//! * [`EvolveTokenSigner::rotate`] — swap to a new key in-process (sync).
//! * [`EvolveTokenSigner::spawn_rotation_poll`] — launch a Tokio background
//!   task that polls a [`sera_secrets::SecretsProvider`] every `interval` and
//!   calls `rotate` when the secret value changes.
//!
//! During verification, if the signature does not match the current key, the
//! verifier tries each history entry whose `rotated_at` is within the
//! configurable grace period (default [`ROTATION_GRACE_SECS`] = 60 s). This
//! prevents in-flight tokens from receiving a 401 immediately after a key
//! swap.
//!
//! The public `sign` and `verify` methods remain **synchronous** so existing
//! route handlers and tests require no changes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sera_auth::CapabilityToken;
use sera_types::evolution::BlastRadius;
use sha2::{Digest, Sha512};

pub use sera_db::proposal_usage::{
    InMemoryProposalUsageStore, PostgresProposalUsageStore, ProposalUsageStore,
};

/// Grace period (seconds) during which a rotated-out key is still accepted for
/// verification. Tokens signed with an old key that arrive within this window
/// will verify successfully; after it expires they are rejected.
pub const ROTATION_GRACE_SECS: u64 = 60;

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

// ── Key and rotation history ───────────────────────────────────────────────

/// A single signing key held in the rotation slot.
#[derive(Clone)]
struct SigningKey {
    secret: Vec<u8>,
}

/// One entry in the rotation history — the key that was **replaced** by a
/// rotation, together with the [`Instant`] at which it was replaced.
struct HistoryEntry {
    key: SigningKey,
    rotated_at: Instant,
}

/// Bounded ring of previously-active keys retained for the grace period.
/// Capacity is fixed at 2: the two most recent former keys.
#[derive(Default)]
struct RotationHistory {
    entries: Vec<HistoryEntry>,
}

impl RotationHistory {
    /// Maximum number of history slots (2 = two previous keys kept).
    const CAPACITY: usize = 2;

    fn push(&mut self, entry: HistoryEntry) {
        if self.entries.len() >= Self::CAPACITY {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// Iterate over keys that are still within the grace period.
    fn active_grace_keys(&self, grace: Duration) -> impl Iterator<Item = &SigningKey> {
        self.entries
            .iter()
            .filter(move |e| e.rotated_at.elapsed() <= grace)
            .map(|e| &e.key)
    }
}

/// HMAC-SHA-512 signer for [`CapabilityToken`]s.
///
/// Tokens are signed against a shared secret held by the gateway. In
/// production this secret should be distinct from the JWT and API-key
/// secrets so compromise of one auth surface does not enable minting evolve
/// tokens. During dev, operators can re-use `SERA_JWT_SECRET` if
/// `SERA_EVOLVE_TOKEN_SECRET` is unset.
///
/// The active signing key is stored behind an `Arc<RwLock<…>>` (std) so live
/// key rotation is possible without a process restart. `sign` and `verify`
/// remain synchronous; the rotation API is async only at the background-task
/// boundary.  See [`EvolveTokenSigner::rotate`] and
/// [`EvolveTokenSigner::spawn_rotation_poll`].
#[derive(Clone)]
pub struct EvolveTokenSigner {
    current: Arc<RwLock<SigningKey>>,
    history: Arc<RwLock<RotationHistory>>,
    grace: Duration,
}

impl EvolveTokenSigner {
    /// Create a new signer from a raw secret. An empty secret produces a
    /// signer that always fails verification with [`EvolveTokenError::EmptySecret`].
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            current: Arc::new(RwLock::new(SigningKey { secret: secret.into() })),
            history: Arc::new(RwLock::new(RotationHistory::default())),
            grace: Duration::from_secs(ROTATION_GRACE_SECS),
        }
    }

    /// Create a signer with a custom grace period. Primarily for tests.
    pub fn with_grace(secret: impl Into<Vec<u8>>, grace: Duration) -> Self {
        Self {
            current: Arc::new(RwLock::new(SigningKey { secret: secret.into() })),
            history: Arc::new(RwLock::new(RotationHistory::default())),
            grace,
        }
    }

    /// Rotate to a new signing key. The previous key is archived in the
    /// rotation history for the configured grace period.
    ///
    /// If `new_secret` is identical to the current secret, the rotation is
    /// treated as a no-op: the history is not updated and the key is not
    /// swapped. This is important when a background poll re-reads an unchanged
    /// secret — repeated no-op calls should not accumulate stale history
    /// entries.
    pub fn rotate(&self, new_secret: Vec<u8>) {
        // Acquire write lock — history first, then current (consistent order).
        let mut history_guard = self.history.write().expect("history RwLock poisoned");
        let mut current_guard = self.current.write().expect("current RwLock poisoned");

        // No-op if secret unchanged.
        if current_guard.secret == new_secret {
            return;
        }

        // Archive the old key.
        history_guard.push(HistoryEntry {
            key: SigningKey { secret: current_guard.secret.clone() },
            rotated_at: Instant::now(),
        });

        // Install new key.
        current_guard.secret = new_secret;
    }

    /// Spawn a Tokio background task that polls `provider` for `secret_name`
    /// every `interval` and calls [`Self::rotate`] when the value changes.
    ///
    /// The spawned task runs until the process exits (there is no explicit
    /// cancellation handle — the task will be dropped when the tokio runtime
    /// shuts down). Errors from the provider are logged at `warn` level and
    /// do not stop the poll loop.
    ///
    /// Returns `None` if `interval` is zero (polling disabled).
    pub fn spawn_rotation_poll(
        &self,
        provider: Arc<dyn sera_secrets::SecretsProvider>,
        interval: Duration,
        secret_name: String,
    ) -> Option<tokio::task::JoinHandle<()>> {
        if interval.is_zero() {
            return None;
        }

        let signer = self.clone();
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                match provider.get_secret(&secret_name).await {
                    Ok(val) => {
                        signer.rotate(val.into_bytes());
                    }
                    Err(e) => {
                        tracing::warn!(
                            secret = %secret_name,
                            error = %e,
                            "evolve-token rotation poll: failed to read secret"
                        );
                    }
                }
            }
        });
        Some(handle)
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

    /// Compute the HMAC-SHA-512 of the canonical bytes under the given secret.
    fn mac_with_key(secret: &[u8], token: &CapabilityToken) -> [u8; 64] {
        let canon = Self::canonical_bytes(token);
        hmac_sha512(secret, &canon)
    }

    /// Compute the HMAC-SHA-512 of the canonical bytes under the signer's
    /// current key. Returns a 64-byte array that can be dropped straight into
    /// [`CapabilityToken::signature`].
    ///
    /// Implements HMAC per RFC 2104 with SHA-512's 128-byte block size.
    pub fn mac(&self, token: &CapabilityToken) -> [u8; 64] {
        let guard = self.current.read().expect("current RwLock poisoned");
        Self::mac_with_key(&guard.secret, token)
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
    ///
    /// If the current key does not verify the signature, the verifier
    /// additionally checks any grace-period keys from the rotation history
    /// before returning [`EvolveTokenError::InvalidSignature`]. This allows
    /// in-flight tokens to survive a key rotation for up to
    /// [`ROTATION_GRACE_SECS`] seconds.
    pub fn verify(
        &self,
        token: &CapabilityToken,
        required: BlastRadius,
    ) -> Result<(), EvolveTokenError> {
        // ── 1. Signature check (current key, then grace-period keys) ──
        let sig_ok = {
            let current_guard = self.current.read().expect("current RwLock poisoned");
            if current_guard.secret.is_empty() {
                return Err(EvolveTokenError::EmptySecret);
            }

            let expected = Self::mac_with_key(&current_guard.secret, token);
            if constant_time_eq_64(&expected, &token.signature) {
                true
            } else {
                // Try grace-period keys from history.
                let history_guard = self.history.read().expect("history RwLock poisoned");
                history_guard
                    .active_grace_keys(self.grace)
                    .any(|k| {
                        let exp = Self::mac_with_key(&k.secret, token);
                        constant_time_eq_64(&exp, &token.signature)
                    })
            }
        };

        if !sig_ok {
            return Err(EvolveTokenError::InvalidSignature);
        }

        // ── 2. Expiry check ──
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if token.expires_at.timestamp_millis() <= now_ms {
            return Err(EvolveTokenError::Expired);
        }

        // ── 3. Scope check ──
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

    // ── Rotation tests ────────────────────────────────────────────────────

    #[test]
    fn rotate_to_same_key_is_noop() {
        // Rotating to the same secret must not add a history entry.
        let signer = EvolveTokenSigner::new(b"key-a".to_vec());
        signer.rotate(b"key-a".to_vec());
        // The history should be empty — no entry should have been pushed.
        let history = signer.history.read().expect("poisoned");
        assert_eq!(history.entries.len(), 0, "same-key rotate must not push history");
    }

    #[test]
    fn sign_rotate_verify_within_grace() {
        // Sign with key A, rotate to key B, verify immediately — should pass
        // because key A is still in the grace-period history.
        let signer = EvolveTokenSigner::new(b"key-a".to_vec());
        let mut tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
        signer.sign(&mut tok);

        signer.rotate(b"key-b".to_vec());

        // Verify must succeed because key A is in history and within grace.
        assert_eq!(
            signer.verify(&tok, BlastRadius::SingleHookConfig),
            Ok(()),
            "token signed with old key must verify within grace period"
        );
    }

    #[test]
    fn sign_rotate_verify_after_grace_fails() {
        // Sign with key A, rotate to key B with a zero grace period — the
        // history entry is immediately expired, so old-key tokens must fail.
        let signer = EvolveTokenSigner::with_grace(b"key-a".to_vec(), Duration::from_secs(0));
        let mut tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
        signer.sign(&mut tok);

        signer.rotate(b"key-b".to_vec());

        // With zero grace, the history entry is already expired.
        assert_eq!(
            signer.verify(&tok, BlastRadius::SingleHookConfig),
            Err(EvolveTokenError::InvalidSignature),
            "token signed with old key must be rejected after grace expires"
        );
    }

    #[test]
    fn concurrent_sign_and_rotate_no_panic() {
        // Spawn N signer threads + M rotator threads; no panics or data races.
        use std::thread;

        let signer = Arc::new(EvolveTokenSigner::new(b"initial".to_vec()));
        let mut handles = vec![];

        // 8 signer threads
        for i in 0..8u32 {
            let s = Arc::clone(&signer);
            handles.push(thread::spawn(move || {
                let mut tok = test_token(&[BlastRadius::SingleHookConfig], 3600);
                tok.id = format!("tok-{i}");
                s.sign(&mut tok);
                // Either Ok or InvalidSignature is acceptable — we just must not panic.
                let _ = s.verify(&tok, BlastRadius::SingleHookConfig);
            }));
        }

        // 4 rotator threads
        for i in 0..4u8 {
            let s = Arc::clone(&signer);
            handles.push(thread::spawn(move || {
                s.rotate(vec![i; 32]);
            }));
        }

        for h in handles {
            h.join().expect("thread panicked");
        }
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
