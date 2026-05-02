# `sera-auth` — Authentication and Authorization for SERA

**Crate:** `rust/crates/sera-auth`
**Type:** library
**Spec:** `docs/plan/specs/SPEC-identity-authz.md`
**Types source:** `rust/crates/sera-auth/src/types.rs`

---

## Overview

`sera-auth` implements SERA's comprehensive authentication and authorization system. It provides:

- **`AuthorizationProvider`** — pluggable Policy Decision Point (PDP) interface
- **`ApiKeyValidator`** — API key validation with Argon2 password hashing
- **`JwtService`** — JWT token issuance and verification (HS256)
- **`AuthMethod`** — authentication method tracking and context
- **`CapabilityToken`** — fine-grained capability-based authorization
- **`CasbinAuthzAdapter`** — RBAC implementation using Casbin
- **`AuthMiddleware`** — axum middleware for request authentication/authorization

This crate handles both internal service identity (JWT tokens) and external operator authentication (API keys, OIDC), with support for capability narrowing and role-based access control.

---

## Architecture: Authentication & Authorization Flow

### Request Authentication Flow

```
  ┌─────────────────────────────────────────────────────────────┐
  │  Incoming Request                                           │
  │                                                             │
  │  Authorization: Bearer sera_key_abc123                      │
  │  OR Authorization: Bearer eyJ0eXAi...                      │
  │  OR Authorization: Basic dXNlcjpwYXNz                      │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  AuthMiddleware (src/middleware.rs)                         │
  │                                                             │
  │  1. Extract Authorization header                            │
  │  2. Determine auth method (API key vs JWT vs Basic)        │
  │  3. Route to appropriate validator                          │
  └─────────────────────────┬───────────────────────────────────┘
                            │
    ┌─────────────────┬─────┴─────┬─────────────────────────────┐
    │                 │           │                             │
    ▼                 ▼           ▼                             ▼
  ┌─────────┐    ┌────────────┐ ┌───────────────┐    ┌────────────────┐
  │ API Key │    │ JWT Token  │ │ Basic Auth    │    │ Capability     │
  │ (Argon2)│    │ (HS256)    │ │ (deprecated)  │    │ Token          │
  └─────────┘    └────────────┘ └───────────────┘    └────────────────┘
    │                 │           │                             │
    └─────────────────┼───────────┼─────────────────────────────┘
                      │           │
                      ▼           ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  ActingContext Creation                                     │
  │                                                             │
  │  ActingContext {                                            │
  │    operator_id: Some("op_123"),                            │
  │    agent_id: None,                                          │
  │    instance_id: None,                                       │
  │    api_key_id: Some("key_456"),                            │
  │    auth_method: AuthMethod::ApiKey,                         │
  │  }                                                          │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Authorization Check (AuthorizationProvider)                │
  │                                                             │
  │  ctx.authorize(                                             │
  │    Action::Read,                                            │
  │    Resource::Agent("sera-analyst"),                         │
  │    acting_context                                           │
  │  ) → AuthzDecision                                          │
  └─────────────────────────┬───────────────────────────────────┘
                            │
      ┌─────────────────────┼─────────────────────────────────┐
      │                     │                                 │
      ▼                     ▼                                 ▼
  ┌─────────┐       ┌───────────────┐              ┌─────────────────┐
  │ Allow   │       │ Deny          │              │ NeedsApproval   │
  │         │       │ (403 error)   │              │ (HITL routing)  │
  └─────────┘       └───────────────┘              └─────────────────┘
      │                     │                                 │
      └─────────────────────┼─────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Route Handler Execution                                    │
  │                                                             │
  │  Request proceeds with validated ActingContext              │
  │  Available via axum extractors                              │
  └─────────────────────────────────────────────────────────────┘
```

### Authorization Provider Architecture

```
  ┌─────────────────────────────────────────────────────────────┐
  │  AuthorizationProvider Trait                               │
  │                                                             │
  │  async fn authorize(                                        │
  │    &self,                                                   │
  │    action: Action,                                          │
  │    resource: Resource,                                      │
  │    context: AuthzContext                                    │
  │  ) -> Result<AuthzDecision, AuthzError>                     │
  └─────────────────────────┬───────────────────────────────────┘
                            │
          ┌─────────────────┼─────────────────┐
          │                 │                 │
          ▼                 ▼                 ▼
  ┌─────────────────┐ ┌────────────────┐ ┌───────────────────┐
  │ DefaultAuthz    │ │ CasbinAuthz    │ │ EnterpriseAuthz   │
  │ Provider        │ │ Adapter        │ │ (AuthZen PDP)     │
  │                 │ │                │ │                   │
  │ Simple RBAC     │ │ Casbin RBAC    │ │ External policy   │
  │ Built-in rules  │ │ Policy files   │ │ service           │
  └─────────────────┘ └────────────────┘ └───────────────────┘
          │                 │                 │
          └─────────────────┼─────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  AuthzDecision                                              │
  │                                                             │
  │  ┌─ Allow                                                   │
  │  ├─ Deny { reason: "Insufficient privileges" }             │
  │  └─ NeedsApproval {                                         │
  │      hint: PendingApprovalHint {                           │
  │        routing_hint: "tier-2-approval",                    │
  │        scope: Some("blast-radius-limited")                 │
  │      }                                                     │
  │    }                                                       │
  └─────────────────────────────────────────────────────────────┘
```

---

## Core Types

### `ActingContext` — Authentication Identity

The authenticated identity for every request.

```rust
pub struct ActingContext {
    pub operator_id: Option<String>,    // Human operator (for API keys)
    pub agent_id: Option<String>,       // Agent identity (for service tokens)
    pub instance_id: Option<String>,    // Instance-specific identity
    pub api_key_id: Option<String>,     // API key identifier
    pub auth_method: AuthMethod,        // How authentication was performed
}

pub enum AuthMethod {
    ApiKey,     // API key validation
    Jwt,        // JWT token verification
    BasicAuth,  // HTTP Basic Auth (deprecated)
    Oidc,       // OpenID Connect (enterprise)
}
```

### `Action` — Authorization Operations

Operations that require authorization checking.

```rust
pub enum Action {
    Read,                             // Read access
    Write,                            // Write access  
    Execute,                          // Execution access
    Admin,                            // Administrative access
    ToolCall(String),                 // Tool execution (e.g., "bash", "web_search")
    SessionOp(String),                // Session operations ("join", "terminate")
    MemoryAccess(String),             // Memory scope access
    ConfigChange(String),             // Configuration modifications
    ProposeChange(BlastRadius),       // Change proposal with blast radius
    ApproveChange(ChangeArtifactId),  // Change artifact approval
}
```

**Usage Examples:**
```rust
Action::Read                                    // Read agent configuration
Action::ToolCall("bash".to_string())           // Execute bash commands
Action::MemoryAccess("tier-2".to_string())     // Access tier-2 memory
Action::ProposeChange(BlastRadius::Limited)     // Propose limited-blast change
Action::ApproveChange(artifact_id)              // Approve specific change
```

### `Resource` — Authorization Targets

Resources that actions are performed against.

```rust
pub enum Resource {
    Session(String),                  // Session resource
    Agent(String),                    // Agent resource
    Tool(String),                     // Tool resource
    Memory(String),                   // Memory resource
    Config(String),                   // Configuration resource
    Workflow(String),                 // Workflow resource
    System,                           // System-wide resource
    ChangeArtifact(ChangeArtifactId), // Change artifact resource
}
```

**Usage Examples:**
```rust
Resource::Agent("sera-analyst".to_string())     // Specific agent
Resource::Tool("bash".to_string())              // Bash tool access
Resource::Memory("tier-2".to_string())          // Tier-2 memory scope
Resource::System                                // System-wide access
```

### `AuthzContext` — Authorization Context

Complete context for authorization decisions.

```rust
pub struct AuthzContext {
    pub acting_context: ActingContext,           // Who is acting
    pub session_key: Option<String>,             // Current session
    pub request_metadata: HashMap<String, Value>, // Additional context
    pub blast_radius: Option<BlastRadius>,       // Change impact scope
}
```

---

## Authentication Methods

### API Key Authentication

Secure API key validation using Argon2 password hashing.

#### `ApiKeyValidator`

```rust
pub struct StoredApiKey {
    pub key_hash_argon2: String,    // Argon2id PHC-format hash
    pub operator_id: String,        // Owner operator ID
    pub key_id: String,            // Unique key identifier
}

pub struct ApiKeyValidator;

impl ApiKeyValidator {
    pub fn validate(
        token: &str, 
        stored_keys: &[StoredApiKey]
    ) -> Result<ActingContext, AuthError> {
        // Constant-time argon2 verification against each stored key
    }
}
```

**Key Features:**
- **Argon2id hashing**: Memory-hard, side-channel resistant
- **PHC string format**: Standard password hash format
- **Constant-time comparison**: Prevents timing attacks
- **No plaintext storage**: Keys are never stored in plaintext

**API Key Format:**
```
sera_key_1234567890abcdef...  // Production keys
sera_bootstrap_dev_123        // Development bootstrap key
```

### JWT Token Authentication

Internal service identity using HS256 JWT tokens.

#### `JwtService`

```rust
pub struct JwtClaims {
    pub sub: String,                     // Subject (operator/agent ID)
    pub iss: String,                     // Issuer (always "sera")
    pub exp: u64,                        // Expiration (unix timestamp)
    pub agent_id: Option<String>,        // Optional agent ID
    pub instance_id: Option<String>,     // Optional instance ID
}

pub struct JwtService {
    secret: String,
}

impl JwtService {
    pub fn new(secret: String) -> Self
    
    pub fn issue(&self, claims: JwtClaims) -> Result<String, AuthError>
    
    pub fn verify(&self, token: &str) -> Result<JwtClaims, AuthError>
    
    pub fn create_operator_token(
        &self, 
        operator_id: String, 
        duration: Duration
    ) -> Result<String, AuthError>
    
    pub fn create_agent_token(
        &self, 
        agent_id: String, 
        instance_id: Option<String>, 
        duration: Duration
    ) -> Result<String, AuthError>
}
```

**JWT Features:**
- **HS256 algorithm**: Symmetric key signing
- **Service identity**: Internal component authentication
- **Configurable expiration**: Flexible token lifetime
- **Standard claims**: Subject, issuer, expiration
- **SERA-specific claims**: Agent ID, instance ID

---

## Authorization System

### `AuthorizationProvider` Trait

Pluggable authorization interface for Policy Decision Points.

```rust
#[async_trait]
pub trait AuthorizationProvider: Send + Sync {
    async fn authorize(
        &self,
        action: Action,
        resource: Resource,
        context: AuthzContext,
    ) -> Result<AuthzDecision, AuthzError>;
    
    async fn batch_authorize(
        &self,
        requests: Vec<(Action, Resource, AuthzContext)>,
    ) -> Result<Vec<AuthzDecision>, AuthzError>;
}
```

### `AuthzDecision` — Authorization Results

```rust
pub enum AuthzDecision {
    Allow,
    Deny { reason: DenyReason },
    NeedsApproval { hint: PendingApprovalHint },
}

pub enum DenyReason {
    InsufficientPrivileges,
    ResourceNotFound,
    BlastRadiusExceeded,
    CapabilityNotGranted(String),
    Custom(String),
}

pub struct PendingApprovalHint {
    pub routing_hint: String,        // Approval queue identifier
    pub scope: Option<String>,       // Optional scope annotation
}
```

### `DefaultAuthzProvider` — Built-in RBAC

Simple role-based access control implementation.

```rust
pub struct DefaultAuthzProvider {
    rules: Vec<AuthzRule>,
    deny_by_default: bool,
}

pub struct AuthzRule {
    pub role_pattern: String,      // Role matcher (e.g., "operator:*")
    pub action_pattern: String,    // Action matcher (e.g., "Read")
    pub resource_pattern: String,  // Resource matcher (e.g., "Agent:*")
    pub decision: AuthzDecision,   // Allow/Deny/NeedsApproval
}
```

**Built-in Rules:**
- Operators can read/write their own agents
- Agents can only access their designated resources
- Administrative actions require approval
- System-wide operations need elevated privileges

### `CasbinAuthzAdapter` — Advanced RBAC

Integration with Casbin for sophisticated policy management.

```rust
pub struct CasbinAuthzAdapter {
    enforcer: Arc<RwLock<casbin::Enforcer>>,
}

impl CasbinAuthzAdapter {
    pub fn new(model_path: &str, policy_path: &str) -> Result<Self, CasbinError>
    
    pub async fn add_policy(
        &self, 
        subject: &str, 
        object: &str, 
        action: &str
    ) -> Result<bool, CasbinError>
    
    pub async fn remove_policy(
        &self, 
        subject: &str, 
        object: &str, 
        action: &str
    ) -> Result<bool, CasbinError>
    
    pub async fn get_roles_for_user(&self, user: &str) -> Result<Vec<String>, CasbinError>
}
```

**Casbin Features:**
- **Model-driven policies**: Flexible policy definition language
- **Role hierarchies**: Inheritance and role composition
- **Attribute-based rules**: Context-aware decision making
- **Policy persistence**: Database-backed policy storage
- **Runtime policy updates**: Dynamic policy modification

---

## Capability-Based Authorization

### `CapabilityToken` — Fine-Grained Permissions

Cryptographically signed tokens for specific capabilities.

```rust
pub struct CapabilityToken {
    pub principal: String,               // Who the token is for
    pub capabilities: Vec<Capability>,   // What actions are allowed
    pub resource_scope: ResourceScope,   // What resources are accessible
    pub expires_at: DateTime<Utc>,      // When the token expires
    pub issued_by: String,               // Who issued the token
    pub signature: String,               // HMAC-SHA256 signature
}

pub struct Capability {
    pub action: String,                  // Action name (e.g., "tool:bash")
    pub constraints: HashMap<String, Value>, // Additional constraints
}

pub enum ResourceScope {
    Global,                              // All resources
    Agent(String),                       // Specific agent
    Session(String),                     // Specific session
    Memory(String),                      // Specific memory scope
    Custom(HashMap<String, Value>),      // Custom scope definition
}
```

**Token Features:**
- **Cryptographic integrity**: HMAC-SHA256 signature verification
- **Fine-grained permissions**: Action-specific capabilities
- **Resource scoping**: Limit access to specific resources
- **Time-bounded**: Automatic expiration
- **Delegatable**: Tokens can be issued by authorized principals

### Capability Token Usage

```rust
use sera_auth::{CapabilityToken, Capability, ResourceScope};

// Create a capability token for tool execution
let token = CapabilityToken {
    principal: "operator:alice".to_string(),
    capabilities: vec![
        Capability {
            action: "tool:bash".to_string(),
            constraints: HashMap::from([
                ("max_duration_secs".to_string(), json!(300)),
                ("network_access".to_string(), json!(false)),
            ]),
        },
        Capability {
            action: "tool:web_search".to_string(),
            constraints: HashMap::new(),
        },
    ],
    resource_scope: ResourceScope::Agent("sera-analyst".to_string()),
    expires_at: Utc::now() + Duration::hours(1),
    issued_by: "sera-gateway".to_string(),
    signature: "...".to_string(),
};

// Validate and use the token
let validation_result = capability_service.validate_token(&token_string)?;
if validation_result.can_perform("tool:bash", &resource) {
    // Execute the action
}
```

---

## Middleware Integration

### `auth_middleware` — Request Authentication

Axum middleware for automatic request authentication.

```rust
pub async fn auth_middleware<B>(
    State(state): State<GatewayState>,
    mut req: Request<B>,
    next: Next<B>,
) -> Result<Response, StatusCode> {
    // 1. Extract Authorization header
    let auth_header = req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());
    
    // 2. Validate credentials
    let acting_context = match auth_header {
        Some(header) if header.starts_with("Bearer sera_key_") => {
            // API key validation
            let token = &header[7..]; // Remove "Bearer "
            ApiKeyValidator::validate(token, &state.stored_keys)?
        },
        Some(header) if header.starts_with("Bearer eyJ") => {
            // JWT token validation
            let token = &header[7..];
            let claims = state.jwt_service.verify(token)?;
            ActingContext::from_jwt_claims(claims)
        },
        _ => return Err(StatusCode::UNAUTHORIZED),
    };
    
    // 3. Insert context into request extensions
    req.extensions_mut().insert(acting_context);
    
    // 4. Proceed to next middleware/handler
    Ok(next.run(req).await)
}
```

**Middleware Features:**
- **Multiple auth methods**: API keys, JWT tokens, capability tokens
- **Request extension**: ActingContext available to handlers
- **Error handling**: Proper HTTP status codes for auth failures
- **Performance**: Efficient credential validation

### Authorization Enforcement

```rust
// In route handlers
pub async fn get_agent(
    Path(agent_id): Path<String>,
    Extension(ctx): Extension<ActingContext>,
    State(state): State<GatewayState>,
) -> Result<Json<Agent>, StatusCode> {
    // Check authorization
    let authz_context = AuthzContext {
        acting_context: ctx,
        session_key: None,
        request_metadata: HashMap::new(),
        blast_radius: None,
    };
    
    let decision = state.authz_provider
        .authorize(
            Action::Read,
            Resource::Agent(agent_id.clone()),
            authz_context,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
    match decision {
        AuthzDecision::Allow => {
            // Proceed with operation
            let agent = state.db.get_agent(&agent_id).await?;
            Ok(Json(agent))
        },
        AuthzDecision::Deny { reason } => {
            tracing::warn!(?reason, "Authorization denied");
            Err(StatusCode::FORBIDDEN)
        },
        AuthzDecision::NeedsApproval { hint } => {
            // Route to HITL approval system
            tracing::info!(?hint, "Request requires approval");
            Err(StatusCode::ACCEPTED) // 202 - request accepted, pending approval
        },
    }
}
```

---

## Error Handling

### `AuthError` — Comprehensive Error Types

```rust
pub enum AuthError {
    Unauthorized,
    InvalidToken { reason: String },
    TokenExpired { expired_at: DateTime<Utc> },
    JwtError(String),
    HashingError(String),
    ConfigurationError { field: String, reason: String },
}

pub enum AuthzError {
    PolicyEngineError(String),
    InvalidResource { resource: String },
    InvalidAction { action: String },
    ContextMissing { field: String },
    ProviderUnavailable,
}

pub enum CapabilityTokenError {
    InvalidSignature,
    TokenExpired { expired_at: DateTime<Utc> },
    InsufficientCapabilities { required: String, available: Vec<String> },
    ScopeViolation { resource: String, scope: String },
}
```

**Error Response Examples:**
```json
{
  "error": {
    "code": "UNAUTHORIZED",
    "message": "Invalid API key"
  }
}

{
  "error": {
    "code": "AUTHORIZATION_DENIED",
    "message": "Insufficient privileges",
    "details": {
      "action": "Execute",
      "resource": "Tool:bash",
      "reason": "User lacks tool execution privileges"
    }
  }
}

{
  "error": {
    "code": "APPROVAL_REQUIRED",
    "message": "Request requires approval",
    "details": {
      "routing_hint": "tier-2-approval",
      "scope": "blast-radius-limited"
    }
  }
}
```

---

## Configuration and Setup

### Authentication Configuration

```rust
pub struct AuthConfig {
    pub jwt_secret: String,              // HS256 secret for JWT tokens
    pub jwt_expiration_hours: u32,       // Default token lifetime
    pub api_key_min_length: usize,       // Minimum API key length
    pub argon2_config: Argon2Config,     // Argon2 hashing parameters
    pub oidc: Option<OidcConfig>,        // OpenID Connect configuration
}

pub struct Argon2Config {
    pub memory_cost: u32,                // Memory usage in KB
    pub time_cost: u32,                  // Number of iterations
    pub parallelism: u32,                // Number of parallel threads
}

pub struct OidcConfig {
    pub issuer_url: String,              // OIDC provider URL
    pub client_id: String,               // OAuth2 client ID
    pub client_secret: String,           // OAuth2 client secret
    pub redirect_uri: String,            // OAuth2 redirect URI
}
```

### Authorization Configuration

```rust
pub struct AuthzConfig {
    pub provider: AuthzProviderConfig,
    pub default_decision: AuthzDecision,
    pub cache_ttl_secs: u32,
}

pub enum AuthzProviderConfig {
    Default {
        rules_file: Option<String>,
        deny_by_default: bool,
    },
    Casbin {
        model_file: String,
        policy_file: String,
        auto_save: bool,
    },
    External {
        endpoint: String,
        auth_token: String,
        timeout_secs: u32,
    },
}
```

---

## Usage Examples

### Basic Authentication Setup

```rust
use sera_auth::{JwtService, ApiKeyValidator, DefaultAuthzProvider};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize JWT service
    let jwt_service = JwtService::new("your-secret-key".to_string());
    
    // Load stored API keys (from database)
    let stored_keys = vec![
        StoredApiKey {
            key_hash_argon2: "$argon2id$v=19$m=4096,t=3,p=1$...".to_string(),
            operator_id: "operator-123".to_string(),
            key_id: "key-456".to_string(),
        },
    ];
    
    // Initialize authorization provider
    let authz_provider = DefaultAuthzProvider::new();
    
    // Use in middleware
    let app = Router::new()
        .route("/api/agents/:id", get(get_agent))
        .layer(middleware::from_fn_with_state(
            (jwt_service, stored_keys, authz_provider),
            auth_middleware
        ));
    
    Ok(())
}
```

### Custom Authorization Provider

```rust
use sera_auth::{AuthorizationProvider, Action, Resource, AuthzContext, AuthzDecision};
use async_trait::async_trait;

pub struct CustomAuthzProvider {
    policy_engine: PolicyEngine,
}

#[async_trait]
impl AuthorizationProvider for CustomAuthzProvider {
    async fn authorize(
        &self,
        action: Action,
        resource: Resource,
        context: AuthzContext,
    ) -> Result<AuthzDecision, AuthzError> {
        // Custom authorization logic
        match (&action, &resource) {
            (Action::Read, Resource::Agent(agent_id)) => {
                if context.acting_context.operator_id.is_some() {
                    Ok(AuthzDecision::Allow)
                } else {
                    Ok(AuthzDecision::Deny {
                        reason: DenyReason::InsufficientPrivileges,
                    })
                }
            },
            (Action::ToolCall(tool), Resource::Agent(_)) => {
                // Tool-specific authorization
                if self.policy_engine.can_use_tool(&context.acting_context, tool) {
                    Ok(AuthzDecision::Allow)
                } else {
                    Ok(AuthzDecision::NeedsApproval {
                        hint: PendingApprovalHint {
                            routing_hint: format!("tool-approval-{}", tool),
                            scope: Some("tool-execution".to_string()),
                        },
                    })
                }
            },
            _ => Ok(AuthzDecision::Deny {
                reason: DenyReason::Custom("Operation not permitted".to_string()),
            }),
        }
    }
    
    async fn batch_authorize(
        &self,
        requests: Vec<(Action, Resource, AuthzContext)>,
    ) -> Result<Vec<AuthzDecision>, AuthzError> {
        let mut decisions = Vec::new();
        for (action, resource, context) in requests {
            decisions.push(self.authorize(action, resource, context).await?);
        }
        Ok(decisions)
    }
}
```

### Capability Token Example

```rust
use sera_auth::{CapabilityToken, CapabilityService};

// Issue a capability token
let capability_service = CapabilityService::new("token-secret".to_string());

let token = capability_service.issue_token(
    "operator:alice".to_string(),
    vec![
        Capability {
            action: "tool:bash".to_string(),
            constraints: HashMap::from([
                ("max_duration_secs".to_string(), json!(300)),
            ]),
        },
    ],
    ResourceScope::Agent("sera-analyst".to_string()),
    Duration::hours(1),
).await?;

println!("Issued token: {}", token);

// Validate and use token
let validation_result = capability_service.validate_token(&token).await?;
if validation_result.can_perform("tool:bash", "sera-analyst") {
    println!("Token allows bash execution on sera-analyst");
}
```

---

## Integration Points

### With `sera-gateway`

- **Request authentication**: Middleware validates all incoming requests
- **Authorization checks**: Route handlers enforce access control
- **Token issuance**: JWT tokens for internal service communication
- **Error responses**: Standardized auth error handling

### With `sera-db`

- **API key storage**: Stored keys persisted in PostgreSQL
- **Policy storage**: Authorization rules and role mappings
- **Audit logging**: Authentication and authorization events
- **Session tracking**: Authentication context persistence

### With `sera-events`

- **Auth events**: Login, logout, permission changes
- **Audit trail**: Comprehensive security event logging
- **Real-time notifications**: Security-relevant event streaming
- **Compliance reporting**: Audit trail for compliance requirements

### With `sera-hitl`

- **Approval routing**: NeedsApproval decisions route to HITL
- **Escalation chains**: Complex approval workflows
- **Approval tracking**: Decision audit and accountability
- **Policy enforcement**: Approved actions are executed

---

## Public API Surface

```rust
// Core authentication
pub use api_key::{ApiKeyValidator, StoredApiKey};
pub use jwt::{JwtService, JwtClaims};
pub use types::{ActingContext, AuthMethod};

// Authorization
pub use authz::{
    AuthorizationProvider, DefaultAuthzProvider, Action, Resource,
    AuthzContext, AuthzDecision, DenyReason, PendingApprovalHint
};

// Capability tokens
pub use capability::{CapabilityToken, CapabilityService, Capability, ResourceScope};

// Casbin integration
pub use casbin_adapter::{CasbinAuthzAdapter, CasbinError};

// Middleware
pub use middleware::auth_middleware;

// Error types
pub use error::{AuthError, AuthzError, CapabilityTokenError};
```

---

## Test Coverage

The test suite covers:

**Authentication:**
- API key validation with valid/invalid keys
- JWT token issuance, verification, and expiration
- Argon2 hashing and verification performance
- Multi-method authentication handling

**Authorization:**
- DefaultAuthzProvider rule evaluation
- Casbin policy engine integration
- Custom authorization provider patterns
- Batch authorization optimization

**Capability Tokens:**
- Token issuance and signature validation
- Capability checking and constraint enforcement
- Resource scope validation and violations
- Token expiration and lifecycle management

**Middleware:**
- Request authentication flow
- Authorization enforcement
- Error handling and status codes
- Performance under load

**Integration:**
- Database persistence of keys and policies
- Event emission for security events
- HITL approval routing
- End-to-end authentication flows

Security-focused tests include timing attack resistance, credential validation edge cases, and authorization bypass attempts.