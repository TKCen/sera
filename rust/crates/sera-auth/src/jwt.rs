//! JWT token issuance and verification using HS256.

use serde::{Deserialize, Serialize};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AuthError;

/// JWT claims for SERA service identity tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject — typically the operator ID or agent ID
    pub sub: String,
    /// Issuer — always "sera" for SERA tokens
    pub iss: String,
    /// Expiration time (unix timestamp in seconds)
    pub exp: u64,
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
}

impl JwtService {
    /// Create a new JWT service with the given HS256 secret.
    pub fn new(secret: String) -> Self {
        Self { secret }
    }

    /// Issue a new JWT token with the given claims.
    pub fn issue(&self, mut claims: JwtClaims) -> Result<String, AuthError> {
        // Ensure issuer is set
        claims.iss = "sera".to_string();

        let key = EncodingKey::from_secret(self.secret.as_bytes());
        encode(&Header::default(), &claims, &key).map_err(|e| {
            AuthError::JwtError(format!("Failed to encode token: {}", e))
        })
    }

    /// Verify and extract claims from a JWT token.
    pub fn verify(&self, token: &str) -> Result<JwtClaims, AuthError> {
        let key = DecodingKey::from_secret(self.secret.as_bytes());
        let validation = Validation::default();

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
#[allow(dead_code)] // used in tests
fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_and_verify_roundtrip() {
        let service = JwtService::new("test-secret-key".to_string());

        let now = current_unix_timestamp();
        let claims = JwtClaims {
            sub: "operator-123".to_string(),
            iss: "sera".to_string(),
            exp: now + 3600, // 1 hour from now
            agent_id: Some("agent-456".to_string()),
            instance_id: Some("inst-789".to_string()),
        };

        // Issue a token
        let token = service.issue(claims.clone()).expect("Failed to issue token");
        assert!(!token.is_empty());

        // Verify the token
        let verified = service.verify(&token).expect("Failed to verify token");
        assert_eq!(verified.sub, claims.sub);
        assert_eq!(verified.iss, "sera");
        assert_eq!(verified.agent_id, claims.agent_id);
        assert_eq!(verified.instance_id, claims.instance_id);
    }

    #[test]
    fn test_verify_expired_token() {
        let service = JwtService::new("test-secret-key".to_string());

        let now = current_unix_timestamp();
        let claims = JwtClaims {
            sub: "operator-123".to_string(),
            iss: "sera".to_string(),
            exp: now - 3600, // 1 hour ago (expired)
            agent_id: None,
            instance_id: None,
        };

        let token = service.issue(claims).expect("Failed to issue token");

        // Verify should fail with ExpiredToken
        let result = service.verify(&token);
        assert!(matches!(result, Err(AuthError::ExpiredToken)));
    }

    #[test]
    fn test_verify_invalid_signature() {
        let service1 = JwtService::new("secret-key-1".to_string());
        let service2 = JwtService::new("secret-key-2".to_string());

        let now = current_unix_timestamp();
        let claims = JwtClaims {
            sub: "operator-123".to_string(),
            iss: "sera".to_string(),
            exp: now + 3600,
            agent_id: None,
            instance_id: None,
        };

        // Issue token with service1
        let token = service1.issue(claims).expect("Failed to issue token");

        // Try to verify with service2 (different secret)
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
        let now = current_unix_timestamp();
        let claims = JwtClaims {
            sub: "test-subject".to_string(),
            iss: "sera".to_string(),
            exp: now + 3600,
            agent_id: None,
            instance_id: None,
        };

        // Default should produce a working service regardless of env var presence.
        let service = JwtService::default();
        let token = service.issue(claims).expect("Failed to issue token via default service");
        let verified = service.verify(&token).expect("Failed to verify token via default service");
        assert_eq!(verified.sub, "test-subject");
    }

    #[test]
    fn test_issuer_always_set() {
        let service = JwtService::new("test-secret-key".to_string());

        let now = current_unix_timestamp();
        let claims = JwtClaims {
            sub: "operator-123".to_string(),
            iss: "wrong-issuer".to_string(),
            exp: now + 3600,
            agent_id: None,
            instance_id: None,
        };

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

        let now = current_unix_timestamp();
        let claims = JwtClaims {
            sub: "operator-123".to_string(),
            iss: "sera".to_string(),
            exp: now + 3600,
            agent_id: None,
            instance_id: None,
        };

        // Forge a token that claims to be RS256 but is encoded with HMAC key
        // material. The jsonwebtoken library should reject this when we decode
        // with the HS256 `DecodingKey`.
        let mut header = Header::default();
        header.alg = Algorithm::HS384; // different algorithm than HS256 default
        let key = EncodingKey::from_secret(secret.as_bytes());
        let forged = encode(&header, &claims, &key).expect("encode forged token");

        // verify() uses `Validation::default()` which expects HS256 — must err.
        let result = service.verify(&forged);
        assert!(
            result.is_err(),
            "token with mismatched algorithm header must be rejected"
        );
    }

    /// A token whose `exp` is exactly `now - 1` (one second in the past) must
    /// be rejected even accounting for the default 60-second leeway. We go
    /// further back to 120 seconds to be unambiguously expired.
    ///
    /// Verifies the expiry path is always exercised, not just the leeway path.
    #[test]
    fn test_expired_token_with_no_leeway_window() {
        let service = JwtService::new("test-secret-key".to_string());

        let now = current_unix_timestamp();
        // Expired 120 seconds ago — well past the default 60-second leeway.
        let claims = JwtClaims {
            sub: "operator-123".to_string(),
            iss: "sera".to_string(),
            exp: now - 120,
            agent_id: None,
            instance_id: None,
        };

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
        // header.payload with no signature segment — jsonwebtoken requires exactly 3 parts
        let result = service.verify(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
             eyJzdWIiOiJ4IiwiZXhwIjo5OTk5OTk5OTk5fQ",
        );
        assert!(result.is_err(), "two-segment token (no signature) must be rejected");
    }

    /// Flipping a single byte in the signature segment must invalidate the token.
    /// This is the canonical tampered-signature test.
    #[test]
    fn test_tampered_signature_rejected() {
        let service = JwtService::new("test-secret-key".to_string());

        let now = current_unix_timestamp();
        let claims = JwtClaims {
            sub: "operator-123".to_string(),
            iss: "sera".to_string(),
            exp: now + 3600,
            agent_id: None,
            instance_id: None,
        };

        let token = service.issue(claims).expect("issue token");

        // Split into three JWT parts and corrupt the signature segment.
        let parts: Vec<&str> = token.splitn(3, '.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 parts");

        // Flip the last character of the signature to guarantee corruption.
        let mut sig = parts[2].to_string();
        let last = sig.pop().unwrap_or('A');
        // Rotate: 'A'→'B', anything else→'A'
        sig.push(if last == 'A' { 'B' } else { 'A' });

        let tampered = format!("{}.{}.{}", parts[0], parts[1], sig);
        let result = service.verify(&tampered);
        assert!(
            matches!(result, Err(AuthError::InvalidToken)),
            "tampered signature must return InvalidToken, got {result:?}"
        );
    }

    /// A token whose payload is missing the required `exp` claim must be rejected.
    ///
    /// jsonwebtoken's `Validation::default()` has `{"exp"}` in
    /// `required_spec_claims` — this confirms that requirement is enforced
    /// before the signature check would even succeed.
    #[test]
    fn test_missing_exp_claim_rejected() {
        use jsonwebtoken::{encode, Header, EncodingKey};

        let secret = "test-secret-key";
        let service = JwtService::new(secret.to_string());

        // A claims struct with no `exp` field — serde will not include it.
        #[derive(serde::Serialize)]
        struct NoExpClaims {
            sub: String,
            iss: String,
        }

        let claims = NoExpClaims {
            sub: "operator-123".to_string(),
            iss: "sera".to_string(),
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

    /// A token issued by service1 must not be verifiable by service2 (different
    /// secret), even when we re-try with a structurally identical but distinct key.
    /// Complements `test_verify_invalid_signature` with a sub-string-length key.
    #[test]
    fn test_key_length_variation_still_rejected() {
        let service_short = JwtService::new("k".to_string());
        let service_long  = JwtService::new("k-extended-with-extra-bytes".to_string());

        let now = current_unix_timestamp();
        let claims = JwtClaims {
            sub: "x".to_string(),
            iss: "sera".to_string(),
            exp: now + 3600,
            agent_id: None,
            instance_id: None,
        };

        let token = service_short.issue(claims).expect("issue");
        let result = service_long.verify(&token);
        assert!(
            result.is_err(),
            "token signed by short key must not verify with longer key"
        );
    }

    /// Verifies that `JwtClaims` are fully round-tripped through serde without
    /// losing optional fields.
    #[test]
    fn test_claims_serde_roundtrip_all_fields() {
        let now = current_unix_timestamp();
        let original = JwtClaims {
            sub: "agent-serde".to_string(),
            iss: "sera".to_string(),
            exp: now + 7200,
            agent_id: Some("ag-001".to_string()),
            instance_id: Some("inst-002".to_string()),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: JwtClaims = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.sub, original.sub);
        assert_eq!(parsed.iss, original.iss);
        assert_eq!(parsed.exp, original.exp);
        assert_eq!(parsed.agent_id, original.agent_id);
        assert_eq!(parsed.instance_id, original.instance_id);
    }

    /// When optional fields are absent, they must deserialise as `None` (not
    /// cause a panic or unexpected default).
    #[test]
    fn test_claims_serde_roundtrip_no_optional_fields() {
        let now = current_unix_timestamp();
        let original = JwtClaims {
            sub: "operator-only".to_string(),
            iss: "sera".to_string(),
            exp: now + 3600,
            agent_id: None,
            instance_id: None,
        };

        let json = serde_json::to_string(&original).expect("serialize");
        // Confirm absent fields are not present in JSON (skip_serializing_if)
        assert!(!json.contains("agent_id"), "absent agent_id must not appear in JSON");
        assert!(!json.contains("instance_id"), "absent instance_id must not appear in JSON");

        let parsed: JwtClaims = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.agent_id, None);
        assert_eq!(parsed.instance_id, None);
    }
}
