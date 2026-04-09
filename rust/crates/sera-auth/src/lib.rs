//! SERA Auth — API key validation, JWT issuance/verification, OIDC support.
//!
//! Split between:
//! - Internal service identity (HS256/RS256 JWTs via `jsonwebtoken`)
//! - External operator auth (OIDC via `openidconnect`, future)

pub mod api_key;
pub mod authz;
pub mod error;
pub mod jwt;
pub mod middleware;
pub mod types;

// Re-export commonly used types
pub use api_key::{ApiKeyValidator, StoredApiKey};
pub use error::AuthError;
pub use jwt::{JwtClaims, JwtService};
pub use middleware::auth_middleware;
pub use types::{ActingContext, AuthMethod};
