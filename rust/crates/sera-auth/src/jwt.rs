//! JWT token issuance and verification using HS256.

use serde::{Deserialize, Serialize};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AuthError;

/// Default issuer label for SERA-issued JWTs.
pub const DEFAULT_JWT_ISSUER: &str = "sera";
/// Default audience for SERA-issued JWTs.
pub const DEFAULT_JWT_AUDIENCE: &str = "sera";
/// Default clock-skew leeway for token `exp`/`nbf` validation, in seconds.
pub const DEFAULT_JWT_LEEWAY_SECS: u64 = 0;

/// JWT claims for SERA service identity tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject — typically the operator ID or agent ID
    pub sub: String,
    /// Issuer — set by `JwtService::issue` to the configured issuer
    pub iss: String,
    /// Audience — set by `JwtService::issue` to the configured audience(s).
    /// Serialized as an array, compatible with RFC 7519 §4.1.3.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aud: Vec<String>,
    /// Expiration time (unix timestamp in seconds)
    pub exp: u64,
    /// Not-before time (unix timestamp in seconds). Tokens are rejected before this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,
    /// Optional agent ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Optional instance ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

/// Service for issuing and verifying JWT tokens with HS256.
pub struct JwtService {
    secret: String,
    issuer: String,
    audience: Vec<String>,
    leeway_secs: u64,
}

impl JwtService {
    /// Create a new JWT service with the given HS256 secret and default
    /// issuer/audience/leeway (`"sera"` / `["sera"]` / `0`).
    pub fn new(secret: String) -> Self {
        Self::new_with_options(
            secret,
            DEFAULT_JWT_ISSUER.to_string(),
            vec![DEFAULT_JWT_AUDIENCE.to_string()],
            DEFAULT_JWT_LEEWAY_SECS,
        )
    }

    /// Create a new JWT service with explicit issuer, audience and clock-skew leeway.
    pub fn new_with_options(
        secret: String,
        issuer: String,
        audience: Vec<String>,
        leeway_secs: u64,
    ) -> Self {
        Self { secret, issuer, audience, leeway_secs }
    }

    /// Configured issuer for tokens minted/verified by this service.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Configured audience list for tokens minted/verified by this service.
    pub fn audience(&self) -> &[String] {
        &self.audience
    }

    /// Issue a new JWT token with the given claims.
    ///
    /// Overrides `iss` and `aud` to match the service configuration. If `nbf`
    /// is not set on the claims, it defaults to the current time so the token
    /// is immediately valid.
    pub fn issue(&self, mut claims: JwtClaims) -> Result<String, AuthError> {
        // Force issuer / audience to match the service configuration so
        // downstream verify() (which calls set_issuer/set_audience) accepts it.
        claims.iss = self.issuer.clone();
        claims.aud = self.audience.clone();
        if claims.nbf.is_none() {
            claims.nbf = Some(current_unix_timestamp());
        }

        let key = EncodingKey::from_secret(self.secret.as_bytes());
        encode(&Header::default(), &claims, &key).map_err(|e| {
            AuthError::JwtError(format!("Failed to encode token: {}", e))
        })
    }

    /// Verify and extract claims from a JWT token.
    ///
    /// Enforces signature, expiry, not-before, issuer and audience. `leeway`
    /// is taken from the service configuration (default `0` seconds).
    pub fn verify(&self, token: &str) -> Result<JwtClaims, AuthError> {
        let key = DecodingKey::from_secret(self.secret.as_bytes());
        let mut validation = Validation::default();
        validation.leeway = self.leeway_secs;
        validation.validate_nbf = true;
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&self.audience);

        decode::<JwtClaims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(AuthError::from)
    }
}

impl Default for JwtService {
    fn default() -> Self {
        let secret = match std::env::var("SERA_JWT_SECRET") {
            Ok(s) if !s.is_empty() => s,
            _ => {
                tracing::warn!(
                    "SERA_JWT_SECRET is not set — generating a random ephemeral JWT secret. \
                     Tokens will not survive process restarts. Set SERA_JWT_SECRET in production."
                );
                let mut bytes = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut bytes);
                hex::encode(bytes)
            }
        };
        Self::new(secret)
    }
}

/// Helper function to get current unix timestamp.
fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_claims(exp_offset: i64) -> JwtClaims {
        let now = current_unix_timestamp();
        let exp = if exp_offset >= 0 {
            now + exp_offset as u64
        } else {
            now.saturating_sub((-exp_offset) as u64)
        };
        JwtClaims {
            sub: "operator-123".to_string(),
            iss: String::new(),
            aud: Vec::new(),
            exp,
            nbf: None,
            agent_id: None,
            instance_id: None,
        }
    }

    #[test]
    fn test_issue_and_verify_roundtrip() {
        let service = JwtService::new("test-secret-key".to_string());

        let mut claims = base_claims(3600);
        claims.agent_id = Some("agent-456".to_string());
        claims.instance_id = Some("inst-789".to_string());

        let token = service.issue(claims.clone()).expect("Failed to issue token");
        assert!(!token.is_empty());

        let verified = service.verify(&token).expect("Failed to verify token");
        assert_eq!(verified.sub, claims.sub);
        assert_eq!(verified.iss, "sera");
        assert_eq!(verified.aud, vec!["sera".to_string()]);
        assert_eq!(verified.agent_id, claims.agent_id);
        assert_eq!(verified.instance_id, claims.instance_id);
        // issue() backfills nbf
        assert!(verified.nbf.is_some());
    }

    #[test]
    fn test_verify_expired_token() {
        let service = JwtService::new("test-secret-key".to_string());

        let claims = base_claims(-3600); // expired 1 hour ago

        let token = service.issue(claims).expect("Failed to issue token");

        let result = service.verify(&token);
        assert!(matches!(result, Err(AuthError::ExpiredToken)));
    }

    #[test]
    fn test_verify_invalid_signature() {
        let service1 = JwtService::new("secret-key-1".to_string());
        let service2 = JwtService::new("secret-key-2".to_string());

        let claims = base_claims(3600);

        let token = service1.issue(claims).expect("Failed to issue token");

        let result = service2.verify(&token);
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_verify_malformed_token() {
        let service = JwtService::new("test-secret-key".to_string());

        let result = service.verify("not.a.valid.token");
        assert!(result.is_err());
    }

    #[test]
    fn default_provider_reads_env_or_generates() {
        let claims = base_claims(3600);

        let service = JwtService::default();
        let token = service.issue(claims).expect("Failed to issue token via default service");
        let verified = service.verify(&token).expect("Failed to verify token via default service");
        assert_eq!(verified.sub, "operator-123");
    }

    #[test]
    fn test_issuer_always_set() {
        let service = JwtService::new("test-secret-key".to_string());

        let mut claims = base_claims(3600);
        claims.iss = "wrong-issuer".to_string();

        let token = service.issue(claims).expect("Failed to issue token");
        let verified = service.verify(&token).expect("Failed to verify token");

        // Issuer should be overridden to "sera"
        assert_eq!(verified.iss, "sera");
    }

    // ── Security-edge JWT tests ───────────────────────────────────────────────

    /// A token signed with a completely different algorithm header (RS256 label
    /// but HS256 key material) must be rejected. jsonwebtoken rejects the
    /// algorithm mismatch before it ever validates the signature, so the result
    /// must be an error — not a successful decode.
    ///
    /// This guards against the classic "alg:none" / algorithm-confusion attack.
    #[test]
    fn test_wrong_algorithm_header_rejected() {
        use jsonwebtoken::{encode, Header, EncodingKey, Algorithm};

        let secret = "shared-secret-32-bytes-long-padded!!";
        let service = JwtService::new(secret.to_string());

        let mut claims = base_claims(3600);
        claims.iss = "sera".to_string();
        claims.aud = vec!["sera".to_string()];

        // Forge a token that claims to be HS384 but is encoded with HMAC key
        // material. The jsonwebtoken library should reject this when we decode
        // with the HS256 `DecodingKey`.
        let header = Header { alg: Algorithm::HS384, ..Header::default() };
        let key = EncodingKey::from_secret(secret.as_bytes());
        let forged = encode(&header, &claims, &key).expect("encode forged token");

        let result = service.verify(&forged);
        assert!(
            result.is_err(),
            "token with mismatched algorithm header must be rejected"
        );
    }

    /// A token whose `exp` is in the past must be rejected with leeway=0 (the
    /// new default). Previously the default 60-second leeway masked this.
    #[test]
    fn test_expired_token_with_no_leeway_window() {
        let service = JwtService::new("test-secret-key".to_string());

        let claims = base_claims(-120); // expired 2 minutes ago

        let token = service.issue(claims).expect("Failed to issue token");
        let result = service.verify(&token);
        assert!(
            matches!(result, Err(AuthError::ExpiredToken)),
            "token expired 120s ago must return ExpiredToken"
        );
    }

    /// An empty string is not a valid JWT and must be rejected immediately.
    #[test]
    fn test_empty_string_rejected() {
        let service = JwtService::new("test-secret-key".to_string());
        let result = service.verify("");
        assert!(result.is_err(), "empty string must not verify");
    }

    /// A JWT with only two segments (header + payload, no signature) must be
    /// rejected. The token below was generated with alg=HS256 but the signature
    /// segment has been removed, leaving only "header.payload".
    #[test]
    fn test_truncated_token_missing_signature_rejected() {
        let service = JwtService::new("test-secret-key".to_string());
        let result = service.verify(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
             eyJzdWIiOiJ4IiwiZXhwIjo5OTk5OTk5OTk5fQ",
        );
        assert!(result.is_err(), "two-segment token (no signature) must be rejected");
    }

    /// Flipping a single byte in the signature segment must invalidate the token.
    #[test]
    fn test_tampered_signature_rejected() {
        let service = JwtService::new("test-secret-key".to_string());

        let claims = base_claims(3600);

        let token = service.issue(claims).expect("issue token");

        let parts: Vec<&str> = token.splitn(3, '.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 parts");

        let mut sig = parts[2].to_string();
        let last = sig.pop().unwrap_or('A');
        sig.push(if last == 'A' { 'B' } else { 'A' });

        let tampered = format!("{}.{}.{}", parts[0], parts[1], sig);
        let result = service.verify(&tampered);
        assert!(
            matches!(result, Err(AuthError::InvalidToken)),
            "tampered signature must return InvalidToken, got {result:?}"
        );
    }

    /// A token whose payload is missing the required `exp` claim must be rejected.
    #[test]
    fn test_missing_exp_claim_rejected() {
        use jsonwebtoken::{encode, Header, EncodingKey};

        let secret = "test-secret-key";
        let service = JwtService::new(secret.to_string());

        #[derive(serde::Serialize)]
        struct NoExpClaims {
            sub: String,
            iss: String,
            aud: Vec<String>,
        }

        let claims = NoExpClaims {
            sub: "operator-123".to_string(),
            iss: "sera".to_string(),
            aud: vec!["sera".to_string()],
        };

        let key = EncodingKey::from_secret(secret.as_bytes());
        let token = encode(&Header::default(), &claims, &key)
            .expect("encode claims without exp");

        let result = service.verify(&token);
        assert!(
            result.is_err(),
            "token without exp must be rejected (exp is required by default Validation)"
        );
    }

    /// A token issued by one service must not be verifiable by another with a
    /// different (even short) key.
    #[test]
    fn test_key_length_variation_still_rejected() {
        let service_short = JwtService::new("k".to_string());
        let service_long  = JwtService::new("k-extended-with-extra-bytes".to_string());

        let claims = base_claims(3600);

        let token = service_short.issue(claims).expect("issue");
        let result = service_long.verify(&token);
        assert!(
            result.is_err(),
            "token signed by short key must not verify with longer key"
        );
    }

    /// `JwtClaims` must round-trip through serde without losing optional fields.
    #[test]
    fn test_claims_serde_roundtrip_all_fields() {
        let now = current_unix_timestamp();
        let original = JwtClaims {
            sub: "agent-serde".to_string(),
            iss: "sera".to_string(),
            aud: vec!["sera".to_string()],
            exp: now + 7200,
            nbf: Some(now),
            agent_id: Some("ag-001".to_string()),
            instance_id: Some("inst-002".to_string()),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: JwtClaims = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.sub, original.sub);
        assert_eq!(parsed.iss, original.iss);
        assert_eq!(parsed.aud, original.aud);
        assert_eq!(parsed.exp, original.exp);
        assert_eq!(parsed.nbf, original.nbf);
        assert_eq!(parsed.agent_id, original.agent_id);
        assert_eq!(parsed.instance_id, original.instance_id);
    }

    /// When optional fields are absent, they must deserialise as `None` / empty
    /// (not cause a panic or unexpected default).
    #[test]
    fn test_claims_serde_roundtrip_no_optional_fields() {
        let now = current_unix_timestamp();
        let original = JwtClaims {
            sub: "operator-only".to_string(),
            iss: "sera".to_string(),
            aud: Vec::new(),
            exp: now + 3600,
            nbf: None,
            agent_id: None,
            instance_id: None,
        };

        let json = serde_json::to_string(&original).expect("serialize");
        assert!(!json.contains("agent_id"), "absent agent_id must not appear in JSON");
        assert!(!json.contains("instance_id"), "absent instance_id must not appear in JSON");
        assert!(!json.contains("\"nbf\""), "absent nbf must not appear in JSON");
        assert!(!json.contains("\"aud\""), "empty aud must not appear in JSON");

        let parsed: JwtClaims = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.nbf, None);
        assert!(parsed.aud.is_empty());
        assert_eq!(parsed.agent_id, None);
        assert_eq!(parsed.instance_id, None);
    }

    // ── sera-9g7p: nbf / issuer / audience / leeway coverage ─────────────────

    /// G1 — A token whose `nbf` is in the future must be rejected (was previously
    /// accepted because `validate_nbf=false` by default).
    #[test]
    fn test_nbf_in_future_rejected() {
        let service = JwtService::new("test-secret-key".to_string());

        let now = current_unix_timestamp();
        let mut claims = base_claims(3600);
        claims.nbf = Some(now + 3600); // not valid for another hour

        let token = service.issue(claims).expect("issue");
        let result = service.verify(&token);
        assert!(
            result.is_err(),
            "token with nbf in the future must be rejected, got {result:?}"
        );
    }

    /// G1 — A token whose `nbf` is in the past must be accepted.
    #[test]
    fn test_nbf_in_past_accepted() {
        let service = JwtService::new("test-secret-key".to_string());

        let now = current_unix_timestamp();
        let mut claims = base_claims(3600);
        claims.nbf = Some(now.saturating_sub(3600)); // became valid an hour ago

        let token = service.issue(claims).expect("issue");
        let verified = service.verify(&token).expect("token with past nbf must verify");
        assert!(verified.nbf.unwrap() <= now);
    }

    /// G1 — `issue()` backfills `nbf` when absent so tokens remain verifiable
    /// even if the caller does not set `nbf`. Legacy claims (pre-nbf) issued
    /// without nbf still deserialise and verify.
    #[test]
    fn test_nbf_absent_accepted_backcompat() {
        use jsonwebtoken::{encode, Header, EncodingKey};

        let secret = "test-secret-key";
        let service = JwtService::new(secret.to_string());

        // Encode a claims struct that simply has no `nbf` field at all, to
        // simulate a token issued by an older codebase.
        #[derive(serde::Serialize)]
        struct LegacyClaims {
            sub: String,
            iss: String,
            aud: Vec<String>,
            exp: u64,
        }

        let now = current_unix_timestamp();
        let legacy = LegacyClaims {
            sub: "legacy".to_string(),
            iss: "sera".to_string(),
            aud: vec!["sera".to_string()],
            exp: now + 3600,
        };

        let key = EncodingKey::from_secret(secret.as_bytes());
        let token = encode(&Header::default(), &legacy, &key).expect("encode legacy");

        let verified = service.verify(&token).expect("legacy token (no nbf) must verify");
        assert_eq!(verified.nbf, None);
    }

    /// G2 — A token with the wrong issuer must be rejected.
    #[test]
    fn test_wrong_issuer_rejected() {
        // Mint a token with issuer "attacker" but verify against a service
        // expecting issuer "sera".
        let secret = "shared-secret";
        let attacker = JwtService::new_with_options(
            secret.to_string(),
            "attacker".to_string(),
            vec!["sera".to_string()],
            0,
        );
        let service = JwtService::new(secret.to_string());

        let claims = base_claims(3600);
        let token = attacker.issue(claims).expect("issue");

        let result = service.verify(&token);
        assert!(
            result.is_err(),
            "token with wrong issuer must be rejected, got {result:?}"
        );
    }

    /// G2 — A token with an audience the service does not expect must be rejected.
    #[test]
    fn test_wrong_audience_rejected() {
        let secret = "shared-secret";
        let attacker = JwtService::new_with_options(
            secret.to_string(),
            "sera".to_string(),
            vec!["other-service".to_string()],
            0,
        );
        let service = JwtService::new(secret.to_string());

        let claims = base_claims(3600);
        let token = attacker.issue(claims).expect("issue");

        let result = service.verify(&token);
        assert!(
            result.is_err(),
            "token with wrong audience must be rejected, got {result:?}"
        );
    }

    /// G3 — A token expired one second ago is rejected when leeway=0 (the new
    /// default). Previously accepted under the implicit 60-second leeway.
    #[test]
    fn test_expired_one_second_with_zero_leeway_rejected() {
        let service = JwtService::new("test-secret-key".to_string());

        let claims = base_claims(-1); // expired 1s ago

        let token = service.issue(claims).expect("issue");
        let result = service.verify(&token);
        assert!(
            matches!(result, Err(AuthError::ExpiredToken)),
            "1s-expired token must be rejected with leeway=0, got {result:?}"
        );
    }

    /// G3 — A token expired beyond the configured leeway must still be rejected.
    #[test]
    fn test_expired_beyond_configured_leeway_rejected() {
        let service = JwtService::new_with_options(
            "test-secret-key".to_string(),
            "sera".to_string(),
            vec!["sera".to_string()],
            30, // 30-second leeway
        );

        let claims = base_claims(-120); // expired 2 minutes ago, > 30s leeway

        let token = service.issue(claims).expect("issue");
        let result = service.verify(&token);
        assert!(
            matches!(result, Err(AuthError::ExpiredToken)),
            "token expired beyond configured leeway must be rejected, got {result:?}"
        );
    }

    /// Back-compat — `JwtService::new(key)` still issues and verifies tokens
    /// using the default issuer/audience/leeway.
    #[test]
    fn test_new_backcompat_roundtrip() {
        let service = JwtService::new("test-secret-key".to_string());

        let claims = base_claims(3600);

        let token = service.issue(claims).expect("issue via new()");
        let verified = service.verify(&token).expect("verify via new()");

        assert_eq!(verified.iss, DEFAULT_JWT_ISSUER);
        assert_eq!(verified.aud, vec![DEFAULT_JWT_AUDIENCE.to_string()]);
        assert_eq!(service.issuer(), DEFAULT_JWT_ISSUER);
        assert_eq!(service.audience(), &[DEFAULT_JWT_AUDIENCE.to_string()]);
    }

    /// Custom issuer/audience pair round-trips when both sides agree.
    #[test]
    fn test_custom_issuer_and_audience_roundtrip() {
        let secret = "shared-secret".to_string();
        let issuer = "sera-enterprise".to_string();
        let audience = vec!["gateway".to_string(), "runtime".to_string()];

        let service = JwtService::new_with_options(
            secret,
            issuer.clone(),
            audience.clone(),
            0,
        );

        let claims = base_claims(3600);

        let token = service.issue(claims).expect("issue");
        let verified = service.verify(&token).expect("verify");

        assert_eq!(verified.iss, issuer);
        assert_eq!(verified.aud, audience);
    }
}
