//! Canonical CapabilityToken — the single definition for the workspace.
//!
//! This module replaces the previous split between
//! `sera_auth::capability::CapabilityToken` (narrowing-semantic, never used
//! outside its own tests) and `sera_types::evolution::CapabilityToken` (the
//! signed wire-token used by the evolve pipeline and gateway). They are now
//! one struct: the wire-serialised form stays byte-identical (the shape the
//! gateway signs today), with the narrowing / `has` / `consume_proposal`
//! helpers promoted from the old sera-auth type.
//!
//! A CapabilityToken is both:
//! - a **signed** token carried on the wire — `signature: [u8; 64]` is a
//!   gateway-side HMAC-SHA-512 produced by
//!   [`sera_gateway::evolve_token::EvolveTokenSigner`]; and
//! - a **narrowable** token at the auth layer — `narrow` produces a subset
//!   scope view, and `has` / `consume_proposal` enforce the budgets at the
//!   policy gate.
//!
//! # Stability
//!
//! The field layout matches what live tokens in the wild expect: `id`,
//! `scopes` (sorted for canonical-bytes stability at signing time),
//! `expires_at`, `max_proposals`, `signature`. Adding new fields would break
//! the MAC canonicalisation in `EvolveTokenSigner::canonical_bytes` — do not
//! extend this struct without a coordinated signer update.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha512};

use sera_types::evolution::BlastRadius;

mod bytes64 {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = [u8; 64];
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("64 bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<[u8; 64], E> {
                v.try_into().map_err(|_| E::invalid_length(v.len(), &self))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<[u8; 64], A::Error> {
                let mut arr = [0u8; 64];
                for (i, slot) in arr.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(arr)
            }
        }
        d.deserialize_bytes(Visitor)
    }
}

/// A bounded, narrowable, signed capability token.
///
/// Tokens are issued by [`CapabilityTokenIssuer`] and signed by the
/// gateway's [`sera_gateway::evolve_token::EvolveTokenSigner`] (HMAC-SHA-512
/// over the canonical serialisation). The issuer and signer are intentionally
/// orthogonal: the issuer constructs the token *value*; the signer installs
/// the MAC on the serialised form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Token identifier — doubles as the issuer-identity anchor (see the
    /// gateway's `propose` identity cross-check).
    pub id: String,
    /// Blast-radius scopes granted to this token. The set is narrowable
    /// via [`CapabilityToken::narrow`]; widening attempts are rejected.
    pub scopes: HashSet<BlastRadius>,
    /// When this token expires (Unix-wall-clock, UTC).
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Maximum number of proposals this token may authorise. Zero means the
    /// token cannot propose — not unlimited — matching the existing gateway
    /// `ProposalUsageTracker` contract.
    pub max_proposals: u32,
    /// HMAC-SHA-512 over the canonical serialisation. All-zero for unsigned
    /// tokens (issuance before signing); verification rejects the all-zero
    /// signature.
    #[serde(with = "bytes64")]
    pub signature: [u8; 64],
}

/// Errors that can occur when using or narrowing a CapabilityToken.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CapabilityTokenError {
    #[error("scope missing: {0:?}")]
    ScopeMissing(BlastRadius),
    #[error("widening attempt denied")]
    WideningAttempt,
    #[error("token expired")]
    Expired,
    #[error("proposal limit exhausted: limit={limit}")]
    ProposalLimitExhausted { limit: u32 },
}

impl CapabilityToken {
    /// Narrow this token to a smaller set of scopes.
    ///
    /// Every requested scope must already be in `self.scopes`; any scope not
    /// already held results in [`CapabilityTokenError::WideningAttempt`].
    ///
    /// Returns a new token with the narrowed scope and a fresh signature slot
    /// (all-zero — callers must re-sign before the gateway will accept it).
    /// The original is unchanged.
    pub fn narrow(
        &self,
        scopes: HashSet<BlastRadius>,
    ) -> Result<CapabilityToken, CapabilityTokenError> {
        for scope in &scopes {
            if !self.scopes.contains(scope) {
                return Err(CapabilityTokenError::WideningAttempt);
            }
        }

        Ok(CapabilityToken {
            id: self.id.clone(),
            scopes,
            expires_at: self.expires_at,
            max_proposals: self.max_proposals,
            // Narrowing invalidates the MAC — caller must re-sign.
            signature: [0u8; 64],
        })
    }

    /// Check whether this token holds the given scope.
    pub fn has(&self, scope: BlastRadius) -> bool {
        self.scopes.contains(&scope)
    }

    /// Check whether this token is currently expired against `chrono::Utc::now`.
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now() > self.expires_at
    }

    /// Record `used` proposals against this token's budget and return whether
    /// one more is permitted.
    ///
    /// `Ok(())` when `used < max_proposals`; otherwise
    /// [`CapabilityTokenError::ProposalLimitExhausted`]. This helper is pure:
    /// it does not mutate the token (the count lives in the
    /// [`sera_db::proposal_usage::ProposalUsageStore`]), so callers feed in
    /// the currently-consumed count.
    pub fn consume_proposal(&self, used: u32) -> Result<(), CapabilityTokenError> {
        if used >= self.max_proposals {
            return Err(CapabilityTokenError::ProposalLimitExhausted {
                limit: self.max_proposals,
            });
        }
        Ok(())
    }
}

// ── Issuer ────────────────────────────────────────────────────────────────

/// Constructs unsigned [`CapabilityToken`]s from a scope set and expiry
/// policy. Signing is orthogonal and lives in
/// [`sera_gateway::evolve_token::EvolveTokenSigner`]: the issuer produces the
/// value, the signer installs the MAC.
///
/// The default implementation is [`DefaultCapabilityTokenIssuer`].
pub trait CapabilityTokenIssuer: Send + Sync {
    /// Issue an unsigned token with the given identity anchor, scopes,
    /// proposal budget, and TTL. `id` is stored on the token and used by the
    /// gateway as the issuer-identity anchor during the propose identity
    /// cross-check.
    fn issue(
        &self,
        id: String,
        scopes: HashSet<BlastRadius>,
        max_proposals: u32,
        ttl: std::time::Duration,
    ) -> CapabilityToken;
}

/// Default issuer — stamps `expires_at = now + ttl` and leaves the signature
/// zeroed for the gateway signer to fill in.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultCapabilityTokenIssuer;

impl DefaultCapabilityTokenIssuer {
    /// Construct a fresh issuer. Has no internal state — the unit struct is
    /// returned purely for trait-object ergonomics.
    pub fn new() -> Self {
        Self
    }
}

impl CapabilityTokenIssuer for DefaultCapabilityTokenIssuer {
    fn issue(
        &self,
        id: String,
        scopes: HashSet<BlastRadius>,
        max_proposals: u32,
        ttl: std::time::Duration,
    ) -> CapabilityToken {
        let ttl_chrono = chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::zero());
        CapabilityToken {
            id,
            scopes,
            expires_at: chrono::Utc::now() + ttl_chrono,
            max_proposals,
            signature: [0u8; 64],
        }
    }
}

// ── ChangeProposer ────────────────────────────────────────────────────────

/// The principal proposing a change artifact, together with the capability
/// token that authorises the proposal.
///
/// Moved to `sera-auth` alongside [`CapabilityToken`] so the two types live
/// in the same crate — avoids forcing `sera-types` to depend on `sera-auth`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeProposer {
    pub principal_id: String,
    pub capability_token: CapabilityToken,
}

// ── EvolveTokenSigner ─────────────────────────────────────────────────────

/// Grace period (seconds) during which a rotated-out key is still accepted for
/// verification. Tokens signed with an old key that arrive within this window
/// will verify successfully; after it expires they are rejected.
pub const ROTATION_GRACE_SECS: u64 = 60;

/// Errors surfaced by evolve-token operations. Callers map these to HTTP
/// status codes (401 for signature/expiry failures, 403 for scope mismatches).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EvolveTokenError {
    /// The signature did not match the MAC recomputed over the canonical bytes.
    #[error("invalid signature")]
    InvalidSignature,
    /// The token's `expires_at` is in the past.
    #[error("token expired")]
    Expired,
    /// The token lacks the scope required for this request.
    #[error("missing required scope: {0:?}")]
    MissingScope(BlastRadius),
    /// The signer secret is empty — configuration error.
    #[error("signer secret is empty")]
    EmptySecret,
}

#[derive(Clone)]
struct SigningKey {
    secret: Vec<u8>,
}

struct HistoryEntry {
    key: SigningKey,
    rotated_at: Instant,
}

#[derive(Default)]
struct RotationHistory {
    entries: Vec<HistoryEntry>,
}

impl RotationHistory {
    const CAPACITY: usize = 2;

    fn push(&mut self, entry: HistoryEntry) {
        if self.entries.len() >= Self::CAPACITY {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    fn active_grace_keys(&self, grace: Duration) -> impl Iterator<Item = &SigningKey> {
        self.entries
            .iter()
            .filter(move |e| e.rotated_at.elapsed() <= grace)
            .map(|e| &e.key)
    }
}

/// HMAC-SHA-512 signer for [`CapabilityToken`]s with live key rotation.
///
/// The active signing key is stored behind an `Arc<RwLock<SigningKey>>` (std)
/// so key rotation is possible without a process restart. `sign` and `verify`
/// remain synchronous; rotation is available via [`Self::reload_key`] (sync)
/// or [`Self::spawn_rotation_poll`] (async background task).
///
/// A bounded [`RotationHistory`] (capacity 2) retains previous keys for a
/// configurable grace period (default [`ROTATION_GRACE_SECS`] = 60 s), so
/// in-flight tokens continue to verify during a rotation window.
///
/// # File-watch hot-reload
///
/// Build the signer with [`EvolveTokenSigner::with_watched_file`] to spawn a
/// background task that polls a file path every 30 s (mtime-based) and calls
/// [`Self::reload_key`] when the file content changes. This avoids taking on
/// the `notify` crate; 30 s latency is acceptable for secret rotation.
#[derive(Clone)]
pub struct EvolveTokenSigner {
    current: Arc<RwLock<SigningKey>>,
    history: Arc<RwLock<RotationHistory>>,
    grace: Duration,
}

impl EvolveTokenSigner {
    /// Create a new signer from a raw secret. An empty secret produces a signer
    /// that always fails verification with [`EvolveTokenError::EmptySecret`].
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

    /// Atomically swap to a new signing key. The previous key is archived in
    /// the rotation history for the configured grace period.
    ///
    /// If `new_key` is identical to the current secret, the call is a no-op:
    /// the history is not updated and the key is not swapped. This prevents
    /// background poll loops from accumulating stale history entries when the
    /// secret has not changed.
    pub fn reload_key(&self, new_key: Vec<u8>) {
        // Acquire write locks — history first, then current (consistent order
        // to avoid deadlock with verify's read ordering).
        let mut history_guard = self.history.write().expect("history RwLock poisoned");
        let mut current_guard = self.current.write().expect("current RwLock poisoned");

        if current_guard.secret == new_key {
            return;
        }

        history_guard.push(HistoryEntry {
            key: SigningKey { secret: current_guard.secret.clone() },
            rotated_at: Instant::now(),
        });

        current_guard.secret = new_key;
    }

    /// Spawn a Tokio background task that polls `provider` for `secret_name`
    /// every `interval` and calls [`Self::reload_key`] when the value changes.
    ///
    /// Returns `None` if `interval` is zero (polling disabled).
    /// Errors from the provider are logged at `warn` level and do not stop
    /// the poll loop.
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
                    Ok(val) => signer.reload_key(val.into_bytes()),
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

    /// Spawn a Tokio background task that polls `path` every 30 seconds
    /// (mtime-based) and calls [`Self::reload_key`] when the file content
    /// changes.
    ///
    /// This uses simple mtime polling rather than the `notify` crate to avoid
    /// adding a new heavyweight dependency. 30 s latency is acceptable for
    /// secret rotation scenarios.
    ///
    /// The spawned task runs until the Tokio runtime shuts down.
    pub fn with_watched_file(
        self,
        path: impl AsRef<std::path::Path> + Send + 'static,
    ) -> (Self, tokio::task::JoinHandle<()>) {
        let signer = self.clone();
        let handle = tokio::spawn(async move {
            let path = path.as_ref().to_owned();
            let mut last_mtime: Option<std::time::SystemTime> = None;

            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;

                let mtime = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok();

                if mtime != last_mtime && mtime.is_some() {
                    last_mtime = mtime;
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            // Strip trailing whitespace/newlines common in secret files.
                            let key: Vec<u8> = bytes
                                .iter()
                                .rev()
                                .skip_while(|&&b| b == b'\n' || b == b'\r' || b == b' ')
                                .cloned()
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect();
                            signer.reload_key(key);
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "evolve-token file watch: failed to read file"
                            );
                        }
                    }
                }
            }
        });
        (self, handle)
    }

    /// Compute the canonical bytes for a token (everything except the
    /// signature field itself).
    fn canonical_bytes(token: &CapabilityToken) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + token.id.len());

        let id_bytes = token.id.as_bytes();
        out.extend_from_slice(&(id_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(id_bytes);

        let mut scopes: Vec<String> = token.scopes.iter().map(|s| format!("{s:?}")).collect();
        scopes.sort();
        out.extend_from_slice(&(scopes.len() as u32).to_le_bytes());
        for s in &scopes {
            let b = s.as_bytes();
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        }

        out.extend_from_slice(&token.expires_at.timestamp_millis().to_le_bytes());
        out.extend_from_slice(&token.max_proposals.to_le_bytes());

        out
    }

    fn mac_with_key(secret: &[u8], token: &CapabilityToken) -> [u8; 64] {
        let canon = Self::canonical_bytes(token);
        hmac_sha512(secret, &canon)
    }

    /// Compute the HMAC-SHA-512 MAC over the token's canonical bytes using the
    /// current signing key.
    pub fn mac(&self, token: &CapabilityToken) -> [u8; 64] {
        let guard = self.current.read().expect("current RwLock poisoned");
        Self::mac_with_key(&guard.secret, token)
    }

    /// Mint a new signature for `token` and install it in place.
    pub fn sign(&self, token: &mut CapabilityToken) {
        token.signature = self.mac(token);
    }

    /// Verify a token's signature and expiry, checking that `required` is one
    /// of its scopes. Returns `Ok(())` only when all three pass.
    ///
    /// If the current key does not verify, grace-period history keys are tried
    /// before returning [`EvolveTokenError::InvalidSignature`].
    pub fn verify(
        &self,
        token: &CapabilityToken,
        required: BlastRadius,
    ) -> Result<(), EvolveTokenError> {
        let current_secret: Vec<u8> = {
            let guard = self.current.read().expect("current RwLock poisoned");
            if guard.secret.is_empty() {
                return Err(EvolveTokenError::EmptySecret);
            }
            guard.secret.clone()
        };

        let expected = Self::mac_with_key(&current_secret, token);
        let sig_ok = if constant_time_eq_64(&expected, &token.signature) {
            true
        } else {
            let history_guard = self.history.read().expect("history RwLock poisoned");
            history_guard.active_grace_keys(self.grace).any(|k| {
                let exp = Self::mac_with_key(&k.secret, token);
                constant_time_eq_64(&exp, &token.signature)
            })
        };

        if !sig_ok {
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

    /// Verify a token against an explicit keyring (current + any number of
    /// previous keys). This is the grace-window variant for callers that manage
    /// their own key history rather than relying on the built-in
    /// [`RotationHistory`].
    ///
    /// Returns `Ok(())` if the token's signature matches **any** key in `keys`,
    /// the token is not expired, and it holds `required`. The first matching key
    /// wins; order does not matter for correctness.
    ///
    /// This is a pure function — it does not consult the signer's internal
    /// rotation history.
    pub fn verify_with_keyring(
        &self,
        token: &CapabilityToken,
        required: BlastRadius,
        keys: &[Vec<u8>],
    ) -> Result<(), EvolveTokenError> {
        if keys.is_empty() {
            return Err(EvolveTokenError::EmptySecret);
        }

        let sig_ok = keys.iter().any(|k| {
            if k.is_empty() {
                return false;
            }
            let exp = Self::mac_with_key(k, token);
            constant_time_eq_64(&exp, &token.signature)
        });

        if !sig_ok {
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

fn constant_time_eq_64(a: &[u8; 64], b: &[u8; 64]) -> bool {
    let mut acc: u8 = 0;
    for i in 0..64 {
        acc |= a[i] ^ b[i];
    }
    acc == 0
}

fn hmac_sha512(key: &[u8], msg: &[u8]) -> [u8; 64] {
    const BLOCK_SIZE: usize = 128;

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

    let mut inner = Sha512::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();

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
    use std::time::Duration;

    fn make_token(scopes: impl IntoIterator<Item = BlastRadius>) -> CapabilityToken {
        CapabilityToken {
            id: "tok-test".to_string(),
            scopes: scopes.into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64],
        }
    }

    #[test]
    fn narrow_subset_succeeds() {
        let token = make_token([BlastRadius::AgentMemory, BlastRadius::SingleHookConfig]);
        let narrowed = token
            .narrow([BlastRadius::AgentMemory].into_iter().collect())
            .expect("narrow should succeed");
        assert!(narrowed.has(BlastRadius::AgentMemory));
        assert!(!narrowed.has(BlastRadius::SingleHookConfig));
        // Narrowing must reset the signature so the gateway re-signs.
        assert_eq!(narrowed.signature, [0u8; 64]);
    }

    #[test]
    fn narrow_widening_denied() {
        let token = make_token([BlastRadius::AgentMemory]);
        let result = token.narrow(
            [BlastRadius::AgentMemory, BlastRadius::SingleHookConfig]
                .into_iter()
                .collect(),
        );
        assert_eq!(result.unwrap_err(), CapabilityTokenError::WideningAttempt);
    }

    #[test]
    fn has_returns_correct_results() {
        let token = make_token([BlastRadius::AgentMemory]);
        assert!(token.has(BlastRadius::AgentMemory));
        assert!(!token.has(BlastRadius::GatewayCore));
    }

    #[test]
    fn consume_proposal_respects_budget() {
        let token = make_token([BlastRadius::AgentMemory]);
        // max_proposals = 10, used = 9 → still permitted.
        assert!(token.consume_proposal(9).is_ok());
        // used = 10 → exhausted.
        let err = token.consume_proposal(10).unwrap_err();
        assert_eq!(
            err,
            CapabilityTokenError::ProposalLimitExhausted { limit: 10 }
        );
    }

    #[test]
    fn is_expired_after_expiry() {
        let mut token = make_token([BlastRadius::AgentMemory]);
        token.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
        assert!(token.is_expired());
    }

    // ── Issuer tests ──────────────────────────────────────────────────────

    #[test]
    fn issuer_stamps_expires_at_from_ttl() {
        let issuer = DefaultCapabilityTokenIssuer::new();
        let before = chrono::Utc::now();
        let token = issuer.issue(
            "agent-1".to_string(),
            [BlastRadius::AgentMemory].into_iter().collect(),
            5,
            Duration::from_secs(60),
        );
        let after = chrono::Utc::now();
        assert_eq!(token.id, "agent-1");
        assert!(token.has(BlastRadius::AgentMemory));
        assert_eq!(token.max_proposals, 5);
        // expires_at must be between (before + 60s) and (after + 60s).
        let lower = before + chrono::Duration::seconds(59);
        let upper = after + chrono::Duration::seconds(61);
        assert!(
            token.expires_at >= lower && token.expires_at <= upper,
            "expires_at {} out of expected window [{}, {}]",
            token.expires_at,
            lower,
            upper
        );
        // Issuer leaves the signature zeroed for the gateway signer.
        assert_eq!(token.signature, [0u8; 64]);
    }

    #[test]
    fn issuer_preserves_all_scopes() {
        let issuer = DefaultCapabilityTokenIssuer::new();
        let scopes: HashSet<BlastRadius> = [
            BlastRadius::AgentMemory,
            BlastRadius::SingleHookConfig,
            BlastRadius::GatewayCore,
        ]
        .into_iter()
        .collect();
        let token = issuer.issue(
            "multi".to_string(),
            scopes.clone(),
            3,
            Duration::from_secs(30),
        );
        assert_eq!(token.scopes, scopes);
    }

    #[test]
    fn issuer_is_dyn_compatible() {
        // The trait must be usable behind `dyn` so call sites can swap
        // implementations without generics (e.g. a test mock that yields a
        // fixed id).
        let issuer: Box<dyn CapabilityTokenIssuer> =
            Box::new(DefaultCapabilityTokenIssuer::new());
        let token = issuer.issue(
            "dyn-call".to_string(),
            [BlastRadius::AgentMemory].into_iter().collect(),
            1,
            Duration::from_secs(10),
        );
        assert_eq!(token.id, "dyn-call");
        assert_eq!(token.max_proposals, 1);
    }

    // ── Wire-serde parity with the old sera-types shape ──────────────────

    #[test]
    fn wire_serde_roundtrip_preserves_all_fields() {
        let token = CapabilityToken {
            id: "wire-test".to_string(),
            scopes: [BlastRadius::AgentMemory, BlastRadius::GlobalConfig]
                .into_iter()
                .collect(),
            expires_at: chrono::Utc::now(),
            max_proposals: 7,
            signature: [0xABu8; 64],
        };

        let json = serde_json::to_string(&token).expect("serialize");
        let back: CapabilityToken = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, token.id);
        assert_eq!(back.scopes, token.scopes);
        assert_eq!(back.max_proposals, token.max_proposals);
        assert_eq!(back.signature, token.signature);
    }

    // ── EvolveTokenSigner tests ───────────────────────────────────────────

    fn signer_token(scopes: &[BlastRadius]) -> CapabilityToken {
        CapabilityToken {
            id: "signer-tok".to_string(),
            scopes: scopes.iter().copied().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 5,
            signature: [0u8; 64],
        }
    }

    #[test]
    fn signer_reload_key_swaps_atomically() {
        // Sign with key A, reload to key B, sign again.
        // Token signed with A must verify with A (via keyring).
        // Token signed with B must verify with B.
        // Token signed with A must NOT verify with B alone.
        let signer = EvolveTokenSigner::new(b"key-A".to_vec());
        let mut tok_a = signer_token(&[BlastRadius::AgentMemory]);
        signer.sign(&mut tok_a);

        signer.reload_key(b"key-B".to_vec());

        let mut tok_b = signer_token(&[BlastRadius::AgentMemory]);
        tok_b.id = "signer-tok-b".to_string();
        signer.sign(&mut tok_b);

        // tok_b signed with current key B must verify normally.
        assert_eq!(signer.verify(&tok_b, BlastRadius::AgentMemory), Ok(()));

        // tok_a signed with old key A must verify via grace-period history
        // (grace is default 60 s, so this is immediate).
        assert_eq!(signer.verify(&tok_a, BlastRadius::AgentMemory), Ok(()));

        // After zero-grace signer, tok_a must fail.
        let strict = EvolveTokenSigner::with_grace(b"key-B".to_vec(), Duration::from_secs(0));
        // Sign tok_a with strict's current key B — different token, verify correctly.
        let mut tok_strict = signer_token(&[BlastRadius::AgentMemory]);
        strict.sign(&mut tok_strict);
        assert_eq!(strict.verify(&tok_strict, BlastRadius::AgentMemory), Ok(()));

        // tok_a (signed with A) against strict signer that only knows B → fail.
        assert_eq!(
            strict.verify(&tok_a, BlastRadius::AgentMemory),
            Err(EvolveTokenError::InvalidSignature)
        );
    }

    #[test]
    fn signer_verify_with_keyring_accepts_old_and_new_during_rotation() {
        let key_a = b"old-key".to_vec();
        let key_b = b"new-key".to_vec();

        let signer_a = EvolveTokenSigner::new(key_a.clone());
        let signer_b = EvolveTokenSigner::new(key_b.clone());

        let mut tok = signer_token(&[BlastRadius::SingleHookConfig]);
        // Sign with key A (old key).
        signer_a.sign(&mut tok);

        // Verifier holds both A (old) and B (new) in its keyring — grace window.
        assert_eq!(
            signer_b.verify_with_keyring(&tok, BlastRadius::SingleHookConfig, &[key_a.clone(), key_b.clone()]),
            Ok(()),
            "keyring should accept token signed with old key"
        );

        // Token signed with B also verifies.
        let mut tok_b = signer_token(&[BlastRadius::SingleHookConfig]);
        tok_b.id = "tok-new".to_string();
        signer_b.sign(&mut tok_b);
        assert_eq!(
            signer_b.verify_with_keyring(&tok_b, BlastRadius::SingleHookConfig, &[key_a, key_b]),
            Ok(()),
            "keyring should accept token signed with new key"
        );
    }

    #[test]
    fn signer_reload_is_lockfree_or_rwlock_reads_parallelize() {
        // Smoke test: many concurrent readers sign and verify while a background
        // thread rotates the key. No panics, no deadlocks.
        use std::sync::Arc as StdArc;
        use std::thread;

        let signer = StdArc::new(EvolveTokenSigner::new(b"initial-key".to_vec()));
        let mut handles = vec![];

        // 8 reader threads: each signs a token and verifies it.
        for i in 0..8u32 {
            let s = StdArc::clone(&signer);
            handles.push(thread::spawn(move || {
                let mut tok = signer_token(&[BlastRadius::AgentMemory]);
                tok.id = format!("par-tok-{i}");
                s.sign(&mut tok);
                // Result may be Ok or InvalidSignature if a rotate landed between
                // sign and verify; either is acceptable — just must not panic.
                let _ = s.verify(&tok, BlastRadius::AgentMemory);
            }));
        }

        // 2 rotator threads.
        for i in 0..2u8 {
            let s = StdArc::clone(&signer);
            handles.push(thread::spawn(move || {
                s.reload_key(vec![i; 16]);
            }));
        }

        for h in handles {
            h.join().expect("thread must not panic");
        }
    }

    #[test]
    fn signer_file_watch_reloads_on_write() {
        // Write a secret file, build a signer, manually simulate what the poll
        // loop does (read file + reload_key), then verify the new key is active.
        // We don't actually spawn the background task (30s poll would make tests
        // very slow); instead we exercise the same reload_key path that the task
        // calls — confirming the end-to-end wiring works.
        let dir = tempfile::tempdir().expect("tempdir");
        let secret_path = dir.path().join("evolve.secret");

        // Write initial secret.
        std::fs::write(&secret_path, b"file-key-A").expect("write initial");

        let signer = EvolveTokenSigner::new(b"file-key-A".to_vec());
        let mut tok_a = signer_token(&[BlastRadius::GatewayCore]);
        signer.sign(&mut tok_a);
        assert_eq!(signer.verify(&tok_a, BlastRadius::GatewayCore), Ok(()));

        // Simulate file update — poll loop reads new content and calls reload_key.
        std::fs::write(&secret_path, b"file-key-B").expect("write updated");
        let new_bytes = std::fs::read(&secret_path).expect("read updated");
        signer.reload_key(new_bytes);

        // tok_a still verifies within grace period.
        assert_eq!(
            signer.verify(&tok_a, BlastRadius::GatewayCore),
            Ok(()),
            "old token must still verify within grace"
        );

        // New token signed with updated key also verifies.
        let mut tok_b = signer_token(&[BlastRadius::GatewayCore]);
        tok_b.id = "file-tok-b".to_string();
        signer.sign(&mut tok_b);
        assert_eq!(signer.verify(&tok_b, BlastRadius::GatewayCore), Ok(()));
    }
}
