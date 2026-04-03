//! SERA Auth — API key validation, JWT issuance/verification, OIDC support.
//!
//! Split between:
//! - Internal service identity (HS256/RS256 JWTs via `jsonwebtoken`)
//! - External operator auth (OIDC via `openidconnect`, future)
