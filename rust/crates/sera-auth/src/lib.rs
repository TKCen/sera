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
    Action, ActionKind, AuthorizationProvider, AuthzContext, AuthzDecision, AuthzError,
    AuthzProviderAdapter, DefaultAuthzProvider, DenyReason, PendingApprovalHint,
    RbacAuthzProvider, Resource, RoleBasedAuthzProvider, RoleBasedAuthzProviderBuilder,
};
pub use capability::{
    CapabilityToken, CapabilityTokenError, CapabilityTokenIssuer, ChangeProposer,
    DefaultCapabilityTokenIssuer,
};
pub use casbin_adapter::{CasbinAuthzAdapter, CasbinError};
pub use error::AuthError;
pub use jwt::{JwtClaims, JwtService};
pub use middleware::auth_middleware;
pub use types::{ActingContext, AuthMethod};
