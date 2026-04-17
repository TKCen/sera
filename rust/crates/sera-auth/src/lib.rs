//! SERA Auth — API key validation, JWT issuance/verification, OIDC support.
//!
//! Split between:
//! - Internal service identity (HS256/RS256 JWTs via `jsonwebtoken`)
//! - External operator auth (OIDC via `openidconnect`, future)
//! - Capability tokens for agent narrowing (Phase 0)
//! - Casbin RBAC adapter (Phase 0)

pub mod api_key;
pub mod authz;
pub mod capability;
pub mod casbin_adapter;
pub mod error;
pub mod jwt;
pub mod middleware;
pub mod types;

// Re-export commonly used types
pub use api_key::{ApiKeyValidator, StoredApiKey};
pub use authz::{
    Action, AuthzContext, AuthzDecision, AuthzError, AuthorizationProvider, DefaultAuthzProvider,
    DenyReason, PendingApprovalHint, RbacAuthzProvider, Resource,
};
pub use capability::{CapabilityToken, CapabilityTokenError};
pub use casbin_adapter::{CasbinAuthzAdapter, CasbinError};
pub use error::AuthError;
pub use jwt::{JwtClaims, JwtService};
pub use middleware::auth_middleware;
pub use types::{ActingContext, AuthMethod};
