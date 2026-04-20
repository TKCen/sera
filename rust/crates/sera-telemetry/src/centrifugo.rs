//! Centrifugo real-time pub/sub client (migrated from `sera-events::centrifugo`).

use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use sera_errors::{SeraError, SeraErrorCode};

#[derive(Debug, Error)]
pub enum CentrifugoError {
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Token generation error: {0}")]
    TokenError(String),

    #[error("API error: {0}")]
    ApiError(String),
}

impl From<CentrifugoError> for SeraError {
    fn from(err: CentrifugoError) -> Self {
        let code = match &err {
            CentrifugoError::HttpError(_) => SeraErrorCode::Unavailable,
            CentrifugoError::TokenError(_) => SeraErrorCode::Internal,
            CentrifugoError::ApiError(_) => SeraErrorCode::Unavailable,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectionClaims {
    sub: String,
    exp: u64,
}

pub struct CentrifugoClient {
    api_url: String,
    api_key: String,
    token_secret: String,
    client: reqwest::Client,
}

impl CentrifugoClient {
    pub fn new(api_url: String, api_key: String, token_secret: String) -> Self {
        Self {
            api_url,
            api_key,
            token_secret,
            client: reqwest::Client::new(),
        }
    }

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
            return Err(CentrifugoError::ApiError(format!("HTTP {}: {}", status, text)));
        }

        Ok(())
    }

    pub fn generate_connection_token(&self, user_id: &str, expire_at: u64) -> Result<String, CentrifugoError> {
        let claims = ConnectionClaims {
            sub: user_id.to_string(),
            exp: expire_at,
        };

        let key = EncodingKey::from_secret(self.token_secret.as_bytes());
        encode(&Header::default(), &claims, &key)
            .map_err(|e| CentrifugoError::TokenError(format!("Failed to encode JWT: {}", e)))
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

        let key = jsonwebtoken::DecodingKey::from_secret(secret.as_bytes());
        let data = decode::<ConnectionClaims>(&token, &key, &jsonwebtoken::Validation::default())
            .expect("Failed to decode token");

        assert_eq!(data.claims.sub, user_id);
        assert_eq!(data.claims.exp, exp);
    }

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

    #[test]
    fn centrifugo_error_display() {
        let e = CentrifugoError::TokenError("bad key".to_string());
        assert_eq!(e.to_string(), "Token generation error: bad key");
    }

    #[test]
    fn centrifugo_token_error_maps_to_internal() {
        let e: SeraError = CentrifugoError::TokenError("key error".to_string()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    #[test]
    fn centrifugo_api_error_maps_to_unavailable() {
        let e: SeraError = CentrifugoError::ApiError("503".to_string()).into();
        assert_eq!(e.code, SeraErrorCode::Unavailable);
    }
}
