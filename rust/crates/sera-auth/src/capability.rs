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
//!   [`EvolveTokenSigner`]; and
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
use sera_types::principal::Principal;
use uuid::Uuid;

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
/// gateway's [`EvolveTokenSigner`] (HMAC-SHA-512
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
    /// Unique-per-instance identifier minted at issuance and at every
    /// `narrow` hop. Distinct from [`CapabilityToken::id`] (which is the
    /// proposal-usage anchor and stays stable across narrow hops): two
    /// siblings narrowed from the same parent share `id` but carry
    /// distinct `instance_id`s, so [`CapabilityToken::parent_id`] can
    /// point unambiguously at one specific parent for cascade-revocation
    /// tree walks (sera-2q6w).
    ///
    /// `None` for tokens deserialised from the legacy chain-less wire
    /// format that predates this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// Chain pointer at the parent token in the delegation tree. `None` for
    /// root tokens issued directly by [`CapabilityTokenIssuer`]; `Some` for
    /// tokens produced by [`CapabilityToken::narrow`].
    ///
    /// Points at the parent's [`CapabilityToken::instance_id`] when the
    /// parent has one (the post-PR shape), and falls back to the parent's
    /// [`CapabilityToken::id`] when the parent is a legacy token without
    /// `instance_id`. The `instance_id` form is what makes branchy
    /// delegation trees walkable for cascade revocation — siblings share
    /// the parent's `id` but get distinct `instance_id`s.
    ///
    /// Legacy serialised tokens that predate this field deserialise with
    /// `parent_id = None`; the canonical bytes used for signing also remain
    /// the v1 layout when no chain field is set, so previously-signed tokens
    /// keep verifying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Principal id (`Principal::id.0`) of the issuer that performed the
    /// most recent narrow. Captures **who** delegated authority through this
    /// hop. `None` for root tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_by: Option<String>,
    /// Hop count along the delegation chain. `0` for root tokens; each
    /// `narrow` increments by one and is bounded by the configured policy
    /// limit (see [`CapabilityToken::narrow`]).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub delegation_depth: u32,
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
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
    /// The narrow would push the chain past the policy's
    /// `max_delegation_depth`. `attempted` is the depth the new token would
    /// have carried; `max` is the policy ceiling.
    #[error("delegation depth exceeded: max={max}, attempted={attempted}")]
    DepthExceeded { max: u32, attempted: u32 },
}

impl CapabilityToken {
    /// Narrow this token to a smaller set of scopes and stamp the next hop
    /// of the delegation chain.
    ///
    /// `issuer` is the principal performing the narrow — recorded as
    /// `delegated_by` on the new token. `max_depth` is the configured
    /// `PrincipalPolicy::max_delegation_depth` for the issuer; the new
    /// token's `delegation_depth = self.delegation_depth + 1` and the
    /// operation is rejected with [`CapabilityTokenError::DepthExceeded`]
    /// when that increment would exceed the cap.
    ///
    /// Every requested scope must already be in `self.scopes`; any scope not
    /// already held results in [`CapabilityTokenError::WideningAttempt`].
    ///
    /// The returned token carries:
    /// - the parent's `id` verbatim — `id` is the proposal-usage anchor
    ///   (`sera_db::proposal_usage::ProposalUsageStore` keys counters by
    ///   `token_id`), so a delegated token must keep sharing the parent's
    ///   counter to avoid resetting the quota on every hop;
    /// - a fresh UUID `instance_id` so siblings narrowed from the same
    ///   parent are unambiguously distinct in cascade-revocation walks;
    /// - `parent_id = Some(self.instance_id)` when the parent carries one
    ///   (the post-PR root shape), falling back to `Some(self.id.clone())`
    ///   for legacy parent tokens without an `instance_id`;
    /// - `delegated_by = Some(issuer.id.0.clone())`;
    /// - `delegation_depth = self.delegation_depth + 1`;
    /// - an all-zero signature slot — callers must re-sign before the gateway
    ///   will accept it. The signer's canonical bytes automatically switch to
    ///   the v2 layout (which covers the chain fields) when any chain field
    ///   is populated.
    ///
    /// The original token is unchanged.
    pub fn narrow(
        &self,
        scopes: HashSet<BlastRadius>,
        issuer: &Principal,
        max_depth: u32,
    ) -> Result<CapabilityToken, CapabilityTokenError> {
        for scope in &scopes {
            if !self.scopes.contains(scope) {
                return Err(CapabilityTokenError::WideningAttempt);
            }
        }

        let new_depth = self.delegation_depth.saturating_add(1);
        if new_depth > max_depth {
            return Err(CapabilityTokenError::DepthExceeded {
                max: max_depth,
                attempted: new_depth,
            });
        }

        let parent_pointer = self.instance_id.clone().unwrap_or_else(|| self.id.clone());

        Ok(CapabilityToken {
            id: self.id.clone(),
            scopes,
            expires_at: self.expires_at,
            max_proposals: self.max_proposals,
            // Narrowing invalidates the MAC — caller must re-sign.
            signature: [0u8; 64],
            instance_id: Some(Uuid::new_v4().to_string()),
            parent_id: Some(parent_pointer),
            delegated_by: Some(issuer.id.0.clone()),
            delegation_depth: new_depth,
        })
    }

    /// Whether this token has any of the v2-only fields populated — used
    /// by [`EvolveTokenSigner`] to decide whether to compute v1 (legacy)
    /// or v2 (chain-aware) canonical bytes. A token with only an
    /// `instance_id` (root tokens minted post-PR) is still v2 because the
    /// MAC must cover the instance id.
    pub fn has_chain(&self) -> bool {
        self.instance_id.is_some()
            || self.parent_id.is_some()
            || self.delegated_by.is_some()
            || self.delegation_depth != 0
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
/// [`EvolveTokenSigner`]: the issuer produces the
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
            instance_id: Some(Uuid::new_v4().to_string()),
            parent_id: None,
            delegated_by: None,
            delegation_depth: 0,
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
    /// The token's `instance_id` (or an ancestor in its delegation chain) has
    /// been recorded in the cascade-revocation store (sera-2q6w).
    #[error("token revoked: {0}")]
    Revoked(String),
    /// The revocation store could not be queried. This is operationally
    /// distinct from a positive revocation hit: callers should not report a
    /// transient DB/store outage as a security revocation decision.
    #[error("revocation lookup failed: {0}")]
    RevocationLookupFailed(String),
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
    ///
    /// Dispatches between two layouts so legacy tokens keep verifying:
    /// - **v1** — the byte-identical layout shipped before the delegation
    ///   chain fields existed. Used when [`CapabilityToken::has_chain`] is
    ///   `false` (no chain fields populated). Already-signed tokens in the
    ///   wild were signed over these bytes; this branch keeps them
    ///   verifiable indefinitely.
    /// - **v2** — v1 bytes followed by a magic separator and the
    ///   length-prefixed chain fields. Used when any of `parent_id`,
    ///   `delegated_by`, or `delegation_depth` is populated. The separator
    ///   is byte-disjoint from a v1 trailer (which always ends with a
    ///   `u32 max_proposals`), so a v1-signed token cannot be silently
    ///   re-interpreted as v2.
    fn canonical_bytes(token: &CapabilityToken) -> Vec<u8> {
        if token.has_chain() {
            Self::canonical_bytes_v2(token)
        } else {
            Self::canonical_bytes_v1(token)
        }
    }

    fn canonical_bytes_v1(token: &CapabilityToken) -> Vec<u8> {
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

    /// Magic separator pinning the v2 canonical-bytes layout. Lives outside
    /// the v1 payload so a hand-crafted v1 token whose `max_proposals`
    /// happens to look like the chain trailer cannot impersonate v2.
    const V2_MAGIC: &'static [u8] = b"sera-cap-v2";

    fn canonical_bytes_v2(token: &CapabilityToken) -> Vec<u8> {
        let mut out = Self::canonical_bytes_v1(token);
        out.extend_from_slice(Self::V2_MAGIC);

        // Order is: instance_id, parent_id, delegated_by, delegation_depth.
        // Each Option carries an explicit presence byte so None and
        // Some("") hash distinctly (see `write_optional_str`).
        write_optional_str(&mut out, token.instance_id.as_deref());
        write_optional_str(&mut out, token.parent_id.as_deref());
        write_optional_str(&mut out, token.delegated_by.as_deref());

        out.extend_from_slice(&token.delegation_depth.to_le_bytes());

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

/// Injective encoding of an optional string into MAC bytes.
///
/// Emits a 1-byte presence tag (`0x00` for `None`, `0x01` for `Some`) followed
/// — only for `Some` — by a `u32` LE length prefix and the UTF-8 body. This
/// keeps `None` and `Some("")` byte-distinct so a v2-signed token cannot have
/// its chain-pointer optionality flipped without invalidating the MAC.
fn write_optional_str(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        None => out.push(0x00),
        Some(s) => {
            out.push(0x01);
            out.extend_from_slice(&(s.len() as u32).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
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
            instance_id: None,
            parent_id: None,
            delegated_by: None,
            delegation_depth: 0,
        }
    }

    fn test_issuer() -> Principal {
        Principal::for_agent("alice", "alice")
    }

    #[test]
    fn narrow_subset_succeeds() {
        let token = make_token([BlastRadius::AgentMemory, BlastRadius::SingleHookConfig]);
        let narrowed = token
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &test_issuer(),
                5,
            )
            .expect("narrow should succeed");
        assert!(narrowed.has(BlastRadius::AgentMemory));
        assert!(!narrowed.has(BlastRadius::SingleHookConfig));
        // Narrowing must reset the signature so the gateway re-signs.
        assert_eq!(narrowed.signature, [0u8; 64]);
        // Chain stamping.
        assert_eq!(narrowed.parent_id.as_deref(), Some("tok-test"));
        assert_eq!(
            narrowed.delegated_by.as_deref(),
            Some(test_issuer().id.0.as_str())
        );
        assert_eq!(narrowed.delegation_depth, 1);
        // `id` is the proposal-usage anchor and MUST stay stable across
        // narrow — minting a fresh id would let a caller reset the quota
        // counter (`sera_db::proposal_usage::ProposalUsageStore` keys by
        // `token_id`) on every delegation hop.
        assert_eq!(narrowed.id, token.id, "narrow must preserve the quota anchor id");
    }

    #[test]
    fn narrow_preserves_proposal_usage_anchor_across_hops() {
        // Quota counters live in `proposal_usage` keyed by `token_id`. A
        // root token and every descendant produced by repeated narrow must
        // share the same `id` so they share the counter — otherwise a
        // caller could narrow once per call and bypass `max_proposals`.
        let root = make_token([BlastRadius::AgentMemory]);
        let alice = Principal::for_agent("alice", "alice");
        let bob = Principal::for_agent("bob", "bob");

        let hop1 = root
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &alice,
                5,
            )
            .expect("hop1");
        let hop2 = hop1
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &bob,
                5,
            )
            .expect("hop2");

        assert_eq!(root.id, hop1.id, "hop1 must share the root's quota id");
        assert_eq!(hop1.id, hop2.id, "hop2 must share the root's quota id");
        // max_proposals also carries forward unchanged so the budget is
        // shared, not re-granted.
        assert_eq!(root.max_proposals, hop2.max_proposals);
    }

    #[test]
    fn narrow_widening_denied() {
        let token = make_token([BlastRadius::AgentMemory]);
        let result = token.narrow(
            [BlastRadius::AgentMemory, BlastRadius::SingleHookConfig]
                .into_iter()
                .collect(),
            &test_issuer(),
            5,
        );
        assert_eq!(result.unwrap_err(), CapabilityTokenError::WideningAttempt);
    }

    #[test]
    fn narrow_accumulates_chain_across_hops() {
        // Two consecutive narrows by two distinct issuers must record the
        // full chain so cascade revocation can walk it. Parent pointers
        // resolve through `instance_id`, which is unique per hop, so the
        // chain stays unambiguous even when the (stable) `id` repeats.
        let issuer = DefaultCapabilityTokenIssuer::new();
        let root = issuer.issue(
            "tok-test".to_string(),
            [BlastRadius::AgentMemory].into_iter().collect(),
            10,
            std::time::Duration::from_secs(3600),
        );
        let alice = Principal::for_agent("alice", "alice");
        let bob = Principal::for_agent("bob", "bob");

        let hop1 = root
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &alice,
                5,
            )
            .expect("hop1 should succeed");
        let hop2 = hop1
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &bob,
                5,
            )
            .expect("hop2 should succeed");

        assert!(root.parent_id.is_none());
        assert_eq!(root.delegation_depth, 0);
        assert!(root.instance_id.is_some(), "issued root must mint instance_id");

        // hop1 chain pointer must resolve to the root's *instance_id*, not
        // its `id` — siblings would otherwise be indistinguishable.
        assert_eq!(hop1.parent_id, root.instance_id);
        assert_eq!(hop1.delegated_by.as_deref(), Some(alice.id.0.as_str()));
        assert_eq!(hop1.delegation_depth, 1);
        assert!(hop1.instance_id.is_some());
        assert_ne!(hop1.instance_id, root.instance_id);

        assert_eq!(hop2.parent_id, hop1.instance_id);
        assert_eq!(hop2.delegated_by.as_deref(), Some(bob.id.0.as_str()));
        assert_eq!(hop2.delegation_depth, 2);
        assert!(hop2.instance_id.is_some());
        assert_ne!(hop2.instance_id, hop1.instance_id);

        // Quota anchor stays stable regardless of chain depth.
        assert_eq!(root.id, hop1.id);
        assert_eq!(hop1.id, hop2.id);
    }

    /// Branchy delegation tree: two siblings narrowed from the same parent
    /// must carry distinct `instance_id`s and identical `parent_id`s, so a
    /// cascade-revocation walker can deterministically distinguish them
    /// even though they share the (stable) quota-anchor `id`.
    #[test]
    fn narrow_branchy_tree_yields_distinct_instance_ids() {
        let issuer = DefaultCapabilityTokenIssuer::new();
        let root = issuer.issue(
            "tok-root".to_string(),
            [BlastRadius::AgentMemory].into_iter().collect(),
            10,
            std::time::Duration::from_secs(3600),
        );
        let alice = Principal::for_agent("alice", "alice");

        let scopes_a: HashSet<BlastRadius> =
            [BlastRadius::AgentMemory].into_iter().collect();
        let sibling_a = root
            .narrow(scopes_a.clone(), &alice, 5)
            .expect("sibling A");
        let sibling_b = root
            .narrow(scopes_a, &alice, 5)
            .expect("sibling B");

        // Quota anchor + parent pointer are identical between siblings…
        assert_eq!(sibling_a.id, sibling_b.id);
        assert_eq!(sibling_a.parent_id, sibling_b.parent_id);
        assert_eq!(sibling_a.parent_id, root.instance_id);

        // …but instance_ids are distinct, so a tree walk that follows
        // `instance_id ← parent_id` distinguishes them.
        assert_ne!(
            sibling_a.instance_id, sibling_b.instance_id,
            "siblings must be distinguishable by instance_id"
        );
        assert!(sibling_a.instance_id.is_some());
        assert!(sibling_b.instance_id.is_some());
    }

    #[test]
    fn narrow_rejects_when_depth_would_exceed_policy() {
        // depth=1 already; max_depth=1 → next hop would be depth=2 → reject.
        let mut token = make_token([BlastRadius::AgentMemory]);
        token.delegation_depth = 1;
        token.parent_id = Some("root".to_string());
        token.delegated_by = Some("agent:root-issuer".to_string());

        let err = token
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &test_issuer(),
                1,
            )
            .expect_err("must reject narrow past max_depth");
        assert_eq!(
            err,
            CapabilityTokenError::DepthExceeded { max: 1, attempted: 2 }
        );

        // Bumping the cap to 2 must let the same narrow through.
        let next = token
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &test_issuer(),
                2,
            )
            .expect("narrow at the cap must succeed");
        assert_eq!(next.delegation_depth, 2);
    }

    #[test]
    fn narrow_rejects_when_root_max_depth_is_zero() {
        // Default-deny policies (ExternalAgent / Service) carry depth 0;
        // no hop is allowed at all.
        let token = make_token([BlastRadius::AgentMemory]);
        let err = token
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &test_issuer(),
                0,
            )
            .expect_err("max_depth=0 must forbid all narrows");
        assert_eq!(
            err,
            CapabilityTokenError::DepthExceeded { max: 0, attempted: 1 }
        );
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
        // Issuer mints a fresh instance_id so the chain pointer in any
        // future narrow hop resolves to a unique parent reference.
        assert!(
            token.instance_id.is_some(),
            "issuer must mint a fresh instance_id for chain audit"
        );
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
            instance_id: None,
            parent_id: None,
            delegated_by: None,
            delegation_depth: 0,
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
            instance_id: None,
            parent_id: None,
            delegated_by: None,
            delegation_depth: 0,
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

    // ── v1 / v2 canonical-bytes coexistence (sera-s64j) ──────────────────

    /// A chain-less token signs / verifies under the v1 canonical-bytes
    /// layout — bit-identical to what was shipped before the chain fields
    /// existed, so already-signed tokens keep working.
    #[test]
    fn signer_v1_canonical_bytes_used_for_chainless_tokens() {
        let signer = EvolveTokenSigner::new(b"k".to_vec());
        let mut tok = signer_token(&[BlastRadius::AgentMemory]);
        // Sanity: nothing on the chain.
        assert!(!tok.has_chain());

        signer.sign(&mut tok);
        assert_eq!(signer.verify(&tok, BlastRadius::AgentMemory), Ok(()));

        // The bytes the signer fed into HMAC must equal the v1 layout
        // exactly (no v2 magic, no chain trailer).
        let bytes = EvolveTokenSigner::canonical_bytes(&tok);
        let v1 = EvolveTokenSigner::canonical_bytes_v1(&tok);
        assert_eq!(bytes, v1);
        assert!(
            !bytes.windows(EvolveTokenSigner::V2_MAGIC.len())
                .any(|w| w == EvolveTokenSigner::V2_MAGIC),
            "v1 bytes must not carry the v2 magic"
        );
    }

    /// A chained token signs / verifies under the v2 canonical-bytes
    /// layout — the chain fields are MAC-covered so any post-hoc tamper
    /// fails verification.
    #[test]
    fn signer_v2_canonical_bytes_cover_chain_fields() {
        let signer = EvolveTokenSigner::new(b"k".to_vec());
        let root = signer_token(&[BlastRadius::AgentMemory]);
        let issuer = Principal::for_agent("alice", "alice");
        let mut narrowed = root
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &issuer,
                3,
            )
            .expect("narrow");

        assert!(narrowed.has_chain(), "narrowed token must report a chain");

        signer.sign(&mut narrowed);
        assert_eq!(signer.verify(&narrowed, BlastRadius::AgentMemory), Ok(()));

        // v2 bytes must include the magic separator and the chain fields.
        let bytes = EvolveTokenSigner::canonical_bytes(&narrowed);
        assert!(
            bytes.windows(EvolveTokenSigner::V2_MAGIC.len())
                .any(|w| w == EvolveTokenSigner::V2_MAGIC),
            "v2 bytes must carry the magic separator"
        );

        // Tampering with delegation_depth post-signature must invalidate
        // the MAC — the chain field is part of canonical bytes.
        let mut tampered = narrowed.clone();
        tampered.delegation_depth = tampered.delegation_depth.saturating_add(1);
        assert_eq!(
            signer.verify(&tampered, BlastRadius::AgentMemory),
            Err(EvolveTokenError::InvalidSignature),
            "depth tamper must invalidate v2 MAC"
        );

        // Tampering with delegated_by similarly fails.
        let mut tampered2 = narrowed.clone();
        tampered2.delegated_by = Some("agent:eve".to_string());
        assert_eq!(
            signer.verify(&tampered2, BlastRadius::AgentMemory),
            Err(EvolveTokenError::InvalidSignature),
            "delegated_by tamper must invalidate v2 MAC"
        );
    }

    /// Chain-less and chained tokens coexist under the same signer key.
    /// A v1 (chain-less) token signed before the chain fields existed
    /// keeps verifying alongside freshly-issued v2 tokens. This is the
    /// load-bearing backwards-compat property for sera-s64j.
    #[test]
    fn signer_v1_v2_coexist_under_same_key() {
        let signer = EvolveTokenSigner::new(b"shared".to_vec());

        // v1 token: legacy shape, no chain.
        let mut v1_tok = signer_token(&[BlastRadius::AgentMemory]);
        signer.sign(&mut v1_tok);
        let v1_sig = v1_tok.signature;

        // v2 token: chain-equipped.
        let issuer = Principal::for_agent("alice", "alice");
        let mut v2_tok = v1_tok
            .narrow(
                [BlastRadius::AgentMemory].into_iter().collect(),
                &issuer,
                5,
            )
            .expect("narrow");
        signer.sign(&mut v2_tok);
        let v2_sig = v2_tok.signature;

        // Both verify.
        assert_eq!(signer.verify(&v1_tok, BlastRadius::AgentMemory), Ok(()));
        assert_eq!(signer.verify(&v2_tok, BlastRadius::AgentMemory), Ok(()));

        // The signatures are distinct — different canonical bytes.
        assert_ne!(v1_sig, v2_sig);

        // Cross-promotion is rejected: copying the v1 signature onto a token
        // that has chain fields populated must fail (the verifier sees the
        // chain, computes v2 bytes, and the v1 MAC won't match).
        let mut promoted = v1_tok.clone();
        promoted.parent_id = Some("forged-parent".to_string());
        promoted.delegation_depth = 1;
        assert_eq!(
            signer.verify(&promoted, BlastRadius::AgentMemory),
            Err(EvolveTokenError::InvalidSignature),
            "v1-signed token cannot be promoted to v2 by adding chain fields"
        );
    }

    /// `None` and `Some("")` for an optional chain field must produce
    /// distinct MAC bytes. `CapabilityToken::id` and `Principal::id` are
    /// unconstrained strings, so empty values are representable; without
    /// an injective Option encoding, a v2-signed token could have its
    /// chain-pointer optionality flipped between missing and empty without
    /// invalidating the MAC, undermining cascade-revocation traversal.
    #[test]
    fn signer_v2_distinguishes_none_from_empty_chain_fields() {
        let signer = EvolveTokenSigner::new(b"shared".to_vec());

        // Build a token with delegation_depth=1 so has_chain() is true even
        // when both Optionals are None — that lets us isolate the encoding
        // of the Option discriminant from the depth field.
        let base_scopes: HashSet<BlastRadius> = [BlastRadius::AgentMemory].into_iter().collect();
        let mk = |parent: Option<&str>, by: Option<&str>| -> CapabilityToken {
            CapabilityToken {
                id: "id-test".to_string(),
                scopes: base_scopes.clone(),
                expires_at: chrono::DateTime::<chrono::Utc>::from_timestamp(1_700_000_000, 0)
                    .unwrap(),
                max_proposals: 5,
                signature: [0u8; 64],
                instance_id: None,
                parent_id: parent.map(|s| s.to_string()),
                delegated_by: by.map(|s| s.to_string()),
                delegation_depth: 1,
            }
        };

        let none_none = mk(None, None);
        let empty_none = mk(Some(""), None);
        let none_empty = mk(None, Some(""));
        let empty_empty = mk(Some(""), Some(""));

        // Recompute canonical bytes for each — they must all differ.
        let cb_nn = EvolveTokenSigner::canonical_bytes(&none_none);
        let cb_en = EvolveTokenSigner::canonical_bytes(&empty_none);
        let cb_ne = EvolveTokenSigner::canonical_bytes(&none_empty);
        let cb_ee = EvolveTokenSigner::canonical_bytes(&empty_empty);
        assert_ne!(cb_nn, cb_en, "None vs Some(\"\") for parent_id must differ");
        assert_ne!(cb_nn, cb_ne, "None vs Some(\"\") for delegated_by must differ");
        assert_ne!(cb_en, cb_ee, "delegated_by Option must distinguish");
        assert_ne!(cb_ne, cb_ee, "parent_id Option must distinguish");

        // Sign each and confirm a signature minted for one variant fails
        // verify against another — proves the MAC actually covers the
        // Option discriminant, not just the body bytes.
        let mut a = none_none.clone();
        signer.sign(&mut a);

        let mut b = empty_none.clone();
        b.signature = a.signature;
        assert_eq!(
            signer.verify(&b, BlastRadius::AgentMemory),
            Err(EvolveTokenError::InvalidSignature),
            "flipping parent_id None→Some(\"\") with the original MAC must fail verify",
        );

        let mut c = none_empty.clone();
        c.signature = a.signature;
        assert_eq!(
            signer.verify(&c, BlastRadius::AgentMemory),
            Err(EvolveTokenError::InvalidSignature),
            "flipping delegated_by None→Some(\"\") with the original MAC must fail verify",
        );
    }

    /// Legacy serialised tokens (no chain fields in JSON) deserialise into
    /// chain-less `CapabilityToken`s and continue to verify under v1
    /// canonical bytes — an external caller persisting tokens written
    /// before this PR must not see them rejected.
    #[test]
    fn signer_verifies_legacy_serialised_token_without_chain_fields() {
        let signer = EvolveTokenSigner::new(b"shared".to_vec());

        // Build, sign, then serialise — this matches a token persisted
        // before the chain fields existed: its serialised form skips the
        // optional chain fields entirely (they default to None / 0).
        let mut tok = signer_token(&[BlastRadius::AgentMemory]);
        signer.sign(&mut tok);
        let json = serde_json::to_string(&tok).expect("serialize");

        // The serialised form must omit the chain fields when they're at
        // their defaults — this is the on-disk shape we have to keep.
        assert!(
            !json.contains("parent_id")
                && !json.contains("delegated_by")
                && !json.contains("delegation_depth"),
            "chain fields must not be emitted when at defaults; got {json}",
        );

        // Round-trip back to a struct (simulating loading from storage)
        // and verify under the same key.
        let reloaded: CapabilityToken =
            serde_json::from_str(&json).expect("deserialize");
        assert!(!reloaded.has_chain());
        assert_eq!(
            signer.verify(&reloaded, BlastRadius::AgentMemory),
            Ok(()),
            "legacy chain-less token must verify"
        );
    }
}
