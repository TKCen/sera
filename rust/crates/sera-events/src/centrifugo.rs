//! Centrifugo real-time pub/sub client.

use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::error::CentrifugoError;

/// Claims for Centrifugo connection token (HS256 JWT).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectionClaims {
    /// Subject (user ID).
    sub: String,
    /// Expiration time (Unix timestamp).
    exp: u64,
}

/// Centrifugo real-time pub/sub client.
pub struct CentrifugoClient {
    api_url: String,
    api_key: String,
    token_secret: String,
    client: reqwest::Client,
}

impl CentrifugoClient {
    /// Create a new Centrifugo client.
    pub fn new(api_url: String, api_key: String, token_secret: String) -> Self {
        Self {
            api_url,
            api_key,
            token_secret,
            client: reqwest::Client::new(),
        }
    }

    /// Publish data to a channel.
    pub async fn publish(&self, channel: &str, data: serde_json::Value) -> Result<(), CentrifugoError> {
        let url = format!("{}/api/publish", self.api_url);

        let body = serde_json::json!({
            "channel": channel,
            "data": data,
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("apikey {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(CentrifugoError::ApiError(format!(
                "HTTP {}: {}",
                status, text
            )));
        }

        Ok(())
    }

    /// Generate a connection token (HS256 JWT).
    pub fn generate_connection_token(&self, user_id: &str, expire_at: u64) -> Result<String, CentrifugoError> {
        let claims = ConnectionClaims {
            sub: user_id.to_string(),
            exp: expire_at,
        };

        let key = EncodingKey::from_secret(self.token_secret.as_bytes());
        encode(&Header::default(), &claims, &key).map_err(|e| {
            CentrifugoError::TokenError(format!("Failed to encode JWT: {}", e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token() {
        let client = CentrifugoClient::new(
            "http://localhost:8000".to_string(),
            "test_key".to_string(),
            "test_secret".to_string(),
        );

        let token = client.generate_connection_token("user-1", 9999999999).unwrap();
        assert!(!token.is_empty());

        // Verify it's a valid JWT (3 parts separated by dots)
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn token_has_correct_claims() {
        use jsonwebtoken::decode;

        let secret = "test_secret";
        let client = CentrifugoClient::new(
            "http://localhost:8000".to_string(),
            "test_key".to_string(),
            secret.to_string(),
        );

        let user_id = "user-123";
        let exp = 9999999999u64;
        let token = client.generate_connection_token(user_id, exp).unwrap();

        // Verify the token
        let key = jsonwebtoken::DecodingKey::from_secret(secret.as_bytes());
        let data = decode::<ConnectionClaims>(
            &token,
            &key,
            &jsonwebtoken::Validation::default(),
        )
        .expect("Failed to decode token");

        assert_eq!(data.claims.sub, user_id);
        assert_eq!(data.claims.exp, exp);
    }

    // ------------------------------------------------------------------
    // Token determinism: same inputs → same JWT header+payload (different
    // signature if secret differs, but same claims structure)
    // ------------------------------------------------------------------
    #[test]
    fn token_is_deterministic_for_same_inputs() {
        let client = CentrifugoClient::new(
            "http://localhost:8000".to_string(),
            "key".to_string(),
            "secret".to_string(),
        );
        let t1 = client.generate_connection_token("u1", 1_000_000).unwrap();
        let t2 = client.generate_connection_token("u1", 1_000_000).unwrap();
        assert_eq!(t1, t2);
    }

    // ------------------------------------------------------------------
    // Different user IDs produce different tokens
    // ------------------------------------------------------------------
    #[test]
    fn different_users_produce_different_tokens() {
        let client = CentrifugoClient::new(
            "http://localhost:8000".to_string(),
            "key".to_string(),
            "secret".to_string(),
        );
        let t1 = client.generate_connection_token("alice", 9_999_999_999).unwrap();
        let t2 = client.generate_connection_token("bob", 9_999_999_999).unwrap();
        assert_ne!(t1, t2);
    }

    // ------------------------------------------------------------------
    // Different expiry values produce different tokens
    // ------------------------------------------------------------------
    #[test]
    fn different_expiry_produces_different_token() {
        let client = CentrifugoClient::new(
            "http://localhost:8000".to_string(),
            "key".to_string(),
            "secret".to_string(),
        );
        let t1 = client.generate_connection_token("user", 1_000).unwrap();
        let t2 = client.generate_connection_token("user", 2_000).unwrap();
        assert_ne!(t1, t2);
    }

    // ------------------------------------------------------------------
    // Token signed with wrong secret fails to decode
    // ------------------------------------------------------------------
    #[test]
    fn token_wrong_secret_fails_decode() {
        use jsonwebtoken::decode;

        let client = CentrifugoClient::new(
            "http://localhost:8000".to_string(),
            "key".to_string(),
            "correct_secret".to_string(),
        );
        let token = client.generate_connection_token("user-1", 9_999_999_999).unwrap();

        let wrong_key = jsonwebtoken::DecodingKey::from_secret(b"wrong_secret");
        let result = decode::<ConnectionClaims>(
            &token,
            &wrong_key,
            &jsonwebtoken::Validation::default(),
        );
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // Publish URL is built from api_url + /api/publish
    // (We can't send HTTP without a server, but we verify the client
    //  is constructed without panic and the field is accessible via the
    //  token-generation path which uses the same struct.)
    // ------------------------------------------------------------------
    #[test]
    fn client_construction_does_not_panic() {
        let client = CentrifugoClient::new(
            "https://centrifugo.example.com".to_string(),
            "api-key-xyz".to_string(),
            "jwt-secret".to_string(),
        );
        // Can generate a token — confirms client is fully initialised
        let token = client.generate_connection_token("u", 9_999_999_999);
        assert!(token.is_ok());
    }

    // ------------------------------------------------------------------
    // ConnectionClaims serde round-trip
    // ------------------------------------------------------------------
    #[test]
    fn connection_claims_serde_roundtrip() {
        let claims = ConnectionClaims {
            sub: "operator-42".to_string(),
            exp: 1_700_000_000,
        };
        let json = serde_json::to_string(&claims).unwrap();
        let parsed: ConnectionClaims = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.sub, claims.sub);
        assert_eq!(parsed.exp, claims.exp);
    }
}
