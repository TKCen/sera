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
}
