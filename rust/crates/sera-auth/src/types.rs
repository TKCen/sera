//! Core authentication types.

use serde::{Deserialize, Serialize};

/// Authentication method used for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    /// API key authentication
    ApiKey,
    /// JWT token authentication
    Jwt,
    /// OIDC authentication (future)
    Oidc,
}

/// Acting context extracted from authenticated requests.
/// Contains the identity of the principal making the request.
#[derive(Debug, Clone)]
pub struct ActingContext {
    /// ID of the operator/user making the request (if applicable)
    pub operator_id: Option<String>,
    /// ID of the agent (if the request is on behalf of an agent)
    pub agent_id: Option<String>,
    /// ID of the agent instance (if the request is for a specific instance)
    pub instance_id: Option<String>,
    /// ID of the API key used (if API key auth was used)
    pub api_key_id: Option<String>,
    /// The method used to authenticate this request
    pub auth_method: AuthMethod,
}

impl ActingContext {
    /// Create a new acting context with the given operator ID.
    pub fn with_operator(operator_id: String) -> Self {
        Self {
            operator_id: Some(operator_id),
            agent_id: None,
            instance_id: None,
            api_key_id: None,
            auth_method: AuthMethod::Jwt,
        }
    }

    /// Create a new acting context with the given agent ID and instance ID.
    pub fn with_agent(agent_id: String, instance_id: String) -> Self {
        Self {
            operator_id: None,
            agent_id: Some(agent_id),
            instance_id: Some(instance_id),
            api_key_id: None,
            auth_method: AuthMethod::Jwt,
        }
    }

    /// Create a new acting context with the given API key ID.
    pub fn with_api_key(api_key_id: String) -> Self {
        Self {
            operator_id: None,
            agent_id: None,
            instance_id: None,
            api_key_id: Some(api_key_id),
            auth_method: AuthMethod::ApiKey,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acting_context_with_operator() {
        let ctx = ActingContext::with_operator("op-123".to_string());
        assert_eq!(ctx.operator_id, Some("op-123".to_string()));
        assert_eq!(ctx.agent_id, None);
        assert_eq!(ctx.instance_id, None);
        assert_eq!(ctx.auth_method, AuthMethod::Jwt);
    }

    #[test]
    fn test_acting_context_with_agent() {
        let ctx = ActingContext::with_agent("agent-123".to_string(), "inst-456".to_string());
        assert_eq!(ctx.agent_id, Some("agent-123".to_string()));
        assert_eq!(ctx.instance_id, Some("inst-456".to_string()));
        assert_eq!(ctx.auth_method, AuthMethod::Jwt);
    }

    #[test]
    fn test_acting_context_with_api_key() {
        let ctx = ActingContext::with_api_key("key-789".to_string());
        assert_eq!(ctx.api_key_id, Some("key-789".to_string()));
        assert_eq!(ctx.auth_method, AuthMethod::ApiKey);
    }
}
