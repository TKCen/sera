# `sera-gateway` — SERA API Gateway and Transport Layer

**Crate:** `rust/crates/sera-gateway`
**Type:** binary + library
**Spec:** `docs/plan/specs/SPEC-gateway.md`
**Types source:** `rust/crates/sera-types/src/envelope.rs`

---

## Overview

`sera-gateway` implements SERA's main API gateway, providing the central hub for agent orchestration and client communication. It provides:

- **REST API Server** — comprehensive HTTP API with ~190 endpoints (axum-based)
- **Transport Layer** — multiple transport mechanisms for agent runtime communication
- **SQ/EQ Pipeline** — submission queue and event queue processing 
- **Envelope Protocol** — standardized message format for agent communication
- **Plugin System** — extensible middleware and processing hooks
- **Kill Switch** — emergency circuit breaker for system-wide safety
- **Generation Tracking** — binary identity and deployment tracking
- **Discord Integration** — native Discord bot support

This crate serves as both a standalone gateway binary (`sera-gateway`) and a library for embedded gateway functionality in other SERA components.

---

## Architecture: Gateway Request Flow

### HTTP Request Lifecycle

```
  ┌─────────────────────────────────────────────────────────────┐
  │  Client Request (HTTP/WebSocket/Discord)                    │
  │                                                             │
  │  POST /api/agents/{agent_id}/chat                           │
  │  Authorization: Bearer sera_key_xxx                         │
  │  Content-Type: application/json                             │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Middleware Stack (src/middleware.rs)                       │
  │                                                             │
  │  1. CORS handling                                           │
  │  2. Authentication (API key/JWT validation)                 │
  │  3. Authorization (action/resource checking)                │
  │  4. Rate limiting and throttling                            │
  │  5. Request tracing and audit logging                       │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Route Handler (src/routes/*)                               │
  │                                                             │
  │  Extract path params, query params, request body           │
  │  Validate input against OpenAPI schema                     │
  │  Apply business logic and transformations                  │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Envelope Creation (src/envelope.rs)                        │
  │                                                             │
  │  Submission {                                               │
  │    session_key: "session_abc123",                           │
  │    input: user_message,                                     │
  │    context: EventContext { agent_id, generation, ... },    │
  │    dedupe: DedupeKey { channel, account, ... },            │
  │    mode: QueueMode::Collect,                                │
  │  }                                                          │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Transport Dispatch (src/transport/*)                       │
  │                                                             │
  │  ┌─ InProcess: Direct function call to runtime              │
  │  ├─ Stdio: Launch child process, JSON over stdin/stdout     │
  │  ├─ WebSocket: Send to WebSocket-connected runtime          │
  │  ├─ GRPC: Send via GRPC to remote runtime                   │
  │  └─ WebhookBack: HTTP callback to external runtime         │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Agent Runtime Processing                                   │
  │                                                             │
  │  • Context assembly and reasoning                           │
  │  • Tool execution and LLM calls                             │
  │  • Response generation                                      │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Event Processing (back to gateway)                        │
  │                                                             │
  │  Event {                                                    │
  │    session_key: "session_abc123",                           │
  │    kind: EventKind::ChatMessage,                            │
  │    payload: agent_response,                                 │
  │    context: EventContext { ... },                          │
  │  }                                                          │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Response Delivery                                          │
  │                                                             │
  │  HTTP: JSON response with 200/4xx/5xx status               │
  │  WebSocket: Real-time event stream                          │
  │  Discord: Message posting to Discord channel               │
  └─────────────────────────────────────────────────────────────┘
```

### SQ/EQ Pipeline Architecture

```
  ┌─────────────────────────────────────────────────────────────┐
  │  Submission Queue (SQ) — Incoming Requests                 │
  │                                                             │
  │  ┌───────────────┬───────────────┬───────────────┐         │
  │  │ Collect Mode  │ Followup Mode │ Steer Mode    │         │
  │  │               │               │               │         │
  │  │ New convs     │ Continuation  │ Operator      │         │
  │  │ Initial msgs  │ responses     │ intervention  │         │
  │  └───────────────┴───────────────┴───────────────┘         │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Deduplication Layer (src/envelope.rs)                     │
  │                                                             │
  │  DedupeKey { channel, account, peer, session_key, msg_id } │
  │  ↓                                                          │
  │  Skip duplicate submissions within time window             │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Agent Runtime (via Transport)                              │
  │                                                             │
  │  1. Session state management                                │
  │  2. Context assembly (persona, tools, memory, etc.)        │
  │  3. LLM reasoning and tool execution                        │
  │  4. Response generation                                     │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Event Queue (EQ) — Agent Responses                        │
  │                                                             │
  │  Event types: ChatMessage, ToolCall, MemoryUpdate,         │
  │               SessionTransition, ErrorResponse, etc.        │
  └─────────────────────────┬───────────────────────────────────┘
                            │
                            ▼
  ┌─────────────────────────────────────────────────────────────┐
  │  Event Delivery                                             │
  │                                                             │
  │  • HTTP clients: JSON response                              │
  │  • WebSocket clients: Real-time streaming                   │
  │  • Discord channels: Message posting                        │
  │  • Webhook callbacks: HTTP delivery                         │
  └─────────────────────────────────────────────────────────────┘
```

---

## Core Components

### Envelope Protocol

The gateway uses a standardized envelope format for all agent communication.

#### `Submission` — Client to Agent

```rust
pub struct Submission {
    pub session_key: String,              // Session identifier
    pub input: String,                    // User message or command
    pub context: EventContext,            // Execution context
    pub dedupe: DedupeKey,               // Deduplication identifier
    pub mode: QueueMode,                 // Processing mode
    pub metadata: HashMap<String, Value>, // Additional context
}
```

#### `Event` — Agent to Client

```rust
pub struct Event {
    pub session_key: String,         // Session identifier  
    pub kind: EventKind,             // Event type
    pub payload: serde_json::Value,  // Event-specific data
    pub context: EventContext,       // Execution context
    pub timestamp: DateTime<Utc>,    // Event creation time
}
```

#### `EventContext` — Shared Context

```rust
pub struct EventContext {
    pub agent_id: String,
    pub session_key: String,
    pub sender: String,
    pub recipient: String,
    pub principal: String,
    pub cause_by: Option<String>,
    pub parent_session_key: Option<String>,
    pub generation: GenerationMarker,
    pub metadata: HashMap<String, Value>,
}
```

### Transport Layer

Multiple transport mechanisms for agent runtime communication.

#### `Transport` Trait

```rust
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_submission(&self, submission: Submission) -> Result<(), TransportError>;
    async fn recv_events(&self) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, TransportError>;
    async fn close(&self) -> Result<(), TransportError>;
}
```

#### Transport Variants

```rust
pub enum AppServerTransport {
    InProcess,                              // Direct function calls (embedded)
    Stdio {                                 // Child process communication
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
    WebSocket {                             // WebSocket connection
        bind: String,
        tls: bool,
    },
    Grpc {                                  // GRPC remote calls
        endpoint: String,
        tls: bool,
    },
    WebhookBack {                           // HTTP callback
        callback_base_url: String,
    },
    Off,                                    // Disabled
}
```

**InProcess Transport:**
- Direct function calls within the same process
- Lowest latency, highest throughput
- Used for embedded runtime integration

**Stdio Transport:**
- Launches agent runtime as child process
- JSON communication over stdin/stdout
- Process isolation with controlled environment

**WebSocket Transport:**
- Real-time bidirectional communication
- Enterprise feature for distributed deployments
- TLS support for secure connections

### Generation Tracking

Binary identity and deployment tracking for system observability.

#### `GenerationMarker`

```rust
pub struct GenerationMarker {
    pub label: String,              // Version label (e.g., "v1.2.3")
    pub binary_identity: String,    // Unique binary identity
    pub started_at: DateTime<Utc>,  // Process start time
}

impl GenerationMarker {
    pub fn current() -> Self {
        // Generates marker from build-time cargo metadata
    }
}
```

Attached to every event and submission for:
- Version tracking across deployments
- Binary identity verification
- Performance correlation with releases
- Rollback and compatibility analysis

### Queue Processing Modes

#### `QueueMode` — Processing Strategy

```rust
pub enum QueueMode {
    Collect,      // New conversations, initial messages
    Followup,     // Conversation continuations  
    Steer,        // Operator interventions
    SteerBacklog, // Delayed operator interventions
    Interrupt,    // High-priority interrupts
}
```

- **Collect**: Normal user interactions, new conversations
- **Followup**: Agent responses and conversation continuations
- **Steer**: Human operator steering and interventions
- **SteerBacklog**: Queued operator actions when agent busy
- **Interrupt**: Emergency or high-priority messages

### Deduplication System

#### `DedupeKey` — Message Deduplication

```rust
pub struct DedupeKey {
    pub channel: String,      // Communication channel (discord, http, etc.)
    pub account: String,      // Account or workspace identifier
    pub peer: String,         // User or client identifier
    pub session_key: String,  // Session identifier
    pub message_id: String,   // Message-specific identifier
}
```

Prevents duplicate processing of identical messages within a time window, essential for:
- Discord message retry scenarios
- Network-level retransmissions  
- Client-side duplicate submissions
- Race conditions in distributed setups

---

## Route Categories

The gateway exposes ~190 HTTP endpoints organized by domain:

### Core Agent Operations
- **`/api/agents/*`** — Agent lifecycle, configuration, chat
- **`/api/sessions/*`** — Session management and state
- **`/api/tasks/*`** — Task creation, claiming, execution

### Memory and Knowledge  
- **`/api/memory/*`** — Memory operations (store, retrieve, search)
- **`/api/knowledge/*`** — Knowledge base operations
- **`/api/embedding/*`** — Embedding generation and search

### Infrastructure
- **`/api/health/*`** — Health checks and system status
- **`/api/heartbeat/*`** — Agent heartbeat and liveness
- **`/api/audit/*`** — Audit trail and compliance logging

### Integration
- **`/api/channels/*`** — Communication channel management
- **`/api/webhooks/*`** — Webhook configuration and delivery
- **`/api/mcp/*`** — Model Context Protocol endpoints
- **`/api/lsp/*`** — Language Server Protocol support

### Security and Auth
- **`/api/auth/*`** — Authentication and authorization
- **`/api/secrets/*`** — Secret management
- **`/api/service_identities/*`** — Service identity management
- **`/api/permission_requests/*`** — Permission request handling

### Operations
- **`/api/config/*`** — Configuration management
- **`/api/schedules/*`** — Schedule and workflow management
- **`/api/metering/*`** — Usage tracking and billing
- **`/api/sandbox/*`** — Sandbox environment management

### OpenAI Compatibility
- **`/v1/chat/completions`** — OpenAI Chat Completions API
- **`/v1/embeddings`** — OpenAI Embeddings API
- **`/v1/models`** — Model listing and capabilities

---

## Kill Switch System

Emergency circuit breaker for system-wide safety.

### `KillSwitch` — Emergency Safety Control

```rust
pub struct KillSwitch {
    state: Arc<RwLock<KillSwitchState>>,
}

pub enum KillSwitchState {
    Normal,                    // System operating normally
    Degraded { reason: String }, // Limited functionality
    Emergency { reason: String }, // Emergency shutdown
}

impl KillSwitch {
    pub async fn check(&self) -> KillSwitchState
    pub async fn trigger_emergency(&self, reason: String)
    pub async fn trigger_degraded(&self, reason: String)  
    pub async fn reset(&self)
}
```

**Emergency Mode:**
- All non-health endpoints return 503 Service Unavailable
- Existing sessions are gracefully terminated
- New requests are rejected immediately
- System enters safe state

**Degraded Mode:**
- Non-critical endpoints disabled
- Rate limiting strictly enforced
- Tool execution may be restricted
- Performance monitoring increased

### Integration Points

Kill switch is checked at multiple levels:
- **Middleware**: Pre-route kill switch validation
- **Route handlers**: Operation-specific safety checks
- **Transport layer**: Agent communication gating
- **Event processing**: Response delivery control

---

## Plugin System

Extensible middleware and processing hooks.

### `Plugin` Trait

```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn init(&mut self, config: serde_json::Value) -> Result<(), PluginError>;
    
    async fn pre_request(
        &self, 
        req: &HttpRequest, 
        ctx: &mut RequestContext
    ) -> Result<(), PluginError>;
    
    async fn post_response(
        &self, 
        req: &HttpRequest, 
        resp: &HttpResponse, 
        ctx: &RequestContext
    ) -> Result<(), PluginError>;
}
```

**Built-in Plugins:**
- **AuthPlugin**: Authentication and authorization
- **RateLimitPlugin**: Request rate limiting
- **AuditPlugin**: Compliance and security logging
- **MetricsPlugin**: Performance and usage metrics
- **CorsPlugin**: Cross-origin request handling

**Plugin Lifecycle:**
1. Registration at startup with configuration
2. `pre_request` hook before route processing
3. `post_response` hook after route completion
4. Graceful shutdown during system termination

---

## Discord Integration

Native Discord bot support with seamless message handling.

### Discord Bot Architecture

```rust
pub struct DiscordHandler {
    gateway_client: GatewayClient,
    bot_config: DiscordBotConfig,
    channel_mappings: HashMap<String, String>,
}

pub struct DiscordBotConfig {
    pub token: String,
    pub application_id: String,
    pub guild_id: Option<String>,
    pub allowed_channels: Vec<String>,
    pub command_prefix: String,
}
```

**Message Flow:**
1. Discord message received via Discord Gateway
2. Convert to SERA `Submission` with Discord-specific context
3. Route through normal gateway processing pipeline
4. Convert agent `Event` responses back to Discord messages
5. Post to Discord channel with proper formatting

**Features:**
- Slash command integration
- Thread conversation tracking
- Emoji reactions for status indication
- Rich embed support for structured responses
- File upload and attachment handling

---

## Error Handling

### `GatewayError` — Comprehensive Error Types

```rust
pub enum GatewayError {
    AuthenticationFailed { reason: String },
    AuthorizationDenied { action: String, resource: String },
    ValidationFailed { field: String, reason: String },
    TransportError { transport: String, reason: String },
    AgentUnavailable { agent_id: String },
    SessionNotFound { session_key: String },
    RateLimitExceeded { limit: u32, window_secs: u32 },
    KillSwitchActivated { mode: String, reason: String },
    InternalError { source: Box<dyn Error + Send + Sync> },
}
```

**Error Response Format:**
```json
{
  "error": {
    "code": "VALIDATION_FAILED",
    "message": "Invalid agent configuration",
    "details": {
      "field": "agent_id",
      "reason": "Agent ID must be a valid identifier"
    }
  }
}
```

**Status Code Mapping:**
- `400 Bad Request`: Validation errors, malformed requests
- `401 Unauthorized`: Authentication failures
- `403 Forbidden`: Authorization denials
- `404 Not Found`: Resource not found (agent, session, etc.)
- `429 Too Many Requests`: Rate limiting
- `503 Service Unavailable`: Kill switch, agent unavailable
- `500 Internal Server Error`: System errors

---

## Configuration and State

### `GatewayState` — Shared Application State

```rust
pub struct GatewayState {
    pub db: Arc<DatabasePool>,
    pub auth: Arc<dyn AuthorizationProvider>,
    pub transports: Arc<TransportRegistry>,
    pub kill_switch: Arc<KillSwitch>,
    pub plugins: Arc<PluginRegistry>,
    pub metrics: Arc<MetricsCollector>,
    pub config: Arc<GatewayConfig>,
}
```

Shared state is injected into all route handlers via axum's state mechanism, providing access to:
- Database connection pool
- Authorization provider
- Transport registry for agent communication
- Kill switch for emergency controls
- Plugin registry for extensibility
- Metrics collection for observability

### Configuration Management

```rust
pub struct GatewayConfig {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub cors: CorsConfig,
    pub rate_limiting: RateLimitConfig,
    pub discord: Option<DiscordBotConfig>,
    pub kill_switch: KillSwitchConfig,
}
```

Configuration loaded from:
1. Environment variables
2. Configuration files (YAML/TOML)
3. Command-line arguments
4. Runtime updates via `/api/config/*` endpoints

---

## Usage Examples

### Basic Gateway Setup

```rust
use sera_gateway::{GatewayState, GatewayConfig, create_app};
use sera_auth::DefaultAuthzProvider;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize components
    let config = GatewayConfig::from_env()?;
    let db = DatabasePool::connect(&config.database_url).await?;
    let auth = Arc::new(DefaultAuthzProvider::new());
    let kill_switch = Arc::new(KillSwitch::new());
    
    // Create shared state
    let state = GatewayState {
        db: Arc::new(db),
        auth,
        kill_switch,
        config: Arc::new(config.clone()),
        transports: Arc::new(TransportRegistry::new()),
        plugins: Arc::new(PluginRegistry::new()),
        metrics: Arc::new(MetricsCollector::new()),
    };
    
    // Create axum application
    let app = create_app(state);
    
    // Start server
    let listener = tokio::net::TcpListener::bind(&config.server.bind).await?;
    println!("Gateway listening on {}", config.server.bind);
    
    axum::serve(listener, app).await?;
    Ok(())
}
```

### Custom Transport Implementation

```rust
use sera_gateway::transport::{Transport, TransportError};
use async_trait::async_trait;

pub struct CustomTransport {
    endpoint: String,
    client: reqwest::Client,
}

#[async_trait]
impl Transport for CustomTransport {
    async fn send_submission(&self, submission: Submission) -> Result<(), TransportError> {
        let response = self.client
            .post(&format!("{}/submissions", self.endpoint))
            .json(&submission)
            .send()
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;
            
        if !response.status().is_success() {
            return Err(TransportError::SendFailed(
                format!("HTTP {}", response.status())
            ));
        }
        
        Ok(())
    }
    
    async fn recv_events(&self) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, TransportError> {
        // Implementation for event streaming
        todo!()
    }
    
    async fn close(&self) -> Result<(), TransportError> {
        // Cleanup resources
        Ok(())
    }
}
```

### Plugin Development

```rust
use sera_gateway::plugin::{Plugin, PluginError};
use sera_gateway::{HttpRequest, HttpResponse, RequestContext};
use async_trait::async_trait;

pub struct CustomAuditPlugin {
    log_level: String,
}

#[async_trait]
impl Plugin for CustomAuditPlugin {
    fn name(&self) -> &str {
        "custom_audit"
    }
    
    fn init(&mut self, config: serde_json::Value) -> Result<(), PluginError> {
        self.log_level = config.get("log_level")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
            .to_string();
        Ok(())
    }
    
    async fn pre_request(
        &self,
        req: &HttpRequest,
        ctx: &mut RequestContext
    ) -> Result<(), PluginError> {
        // Log incoming request
        tracing::info!(
            level = %self.log_level,
            method = %req.method(),
            path = %req.uri().path(),
            "Incoming request"
        );
        
        // Add custom context
        ctx.insert("audit_timestamp".to_string(), chrono::Utc::now());
        Ok(())
    }
    
    async fn post_response(
        &self,
        req: &HttpRequest,
        resp: &HttpResponse,
        ctx: &RequestContext
    ) -> Result<(), PluginError> {
        // Log response
        let start_time = ctx.get("audit_timestamp").unwrap();
        let duration = chrono::Utc::now() - start_time;
        
        tracing::info!(
            method = %req.method(),
            path = %req.uri().path(),
            status = resp.status().as_u16(),
            duration_ms = duration.num_milliseconds(),
            "Request completed"
        );
        
        Ok(())
    }
}
```

---

## Integration Points

### With `sera-auth`

Gateway integrates with the auth crate for:
- API key validation in middleware
- JWT token issuance and verification
- Authorization policy enforcement
- Principal identity management

### With `sera-db`

Database integration for:
- Session state persistence
- Agent configuration storage
- Audit trail logging
- Metrics and analytics data

### With `sera-events`

Event system integration:
- Publishing gateway lifecycle events
- Agent runtime event routing
- Real-time event streaming to clients
- Event-driven workflow triggers

### With `sera-runtime`

Runtime communication via transport layer:
- Submission forwarding to agents
- Event reception from agent processing
- Session state coordination
- Context sharing and synchronization

---

## Public API Surface

```rust
// Core gateway application
pub use app::create_app;
pub use state::GatewayState;
pub use config::GatewayConfig;

// Envelope protocol
pub use envelope::{
    EventContext, GenerationMarker, DedupeKey, QueueMode, WorkerFailureKind
};

// Transport layer
pub use transport::{
    Transport, AppServerTransport, TransportConfig, TransportError
};

// Plugin system
pub use plugin::{Plugin, PluginError, PluginRegistry};

// Kill switch
pub use kill_switch::{KillSwitch, KillSwitchState};

// Discord integration
pub use discord::{DiscordHandler, DiscordBotConfig};

// Error handling
pub use error::GatewayError;

// Middleware
pub use middleware::{auth_middleware, cors_middleware, rate_limit_middleware};

// Re-exports from sera-types
pub use sera_types::envelope::{Submission, Event, EventKind, Op};
pub use sera_types::harness::AgentHarness;
```

---

## Test Coverage

The test suite covers:

**Transport Layer:**
- InProcess transport direct function calls
- Stdio transport child process communication
- WebSocket transport real-time messaging
- Transport error handling and recovery

**Envelope Protocol:**
- Submission serialization and validation
- Event routing and delivery
- Deduplication key generation and collision handling
- Context propagation and metadata handling

**Route Testing:**
- All ~190 API endpoints with valid/invalid inputs
- Authentication and authorization enforcement
- Request validation and error responses
- OpenAPI schema compliance

**Plugin System:**
- Plugin lifecycle (init, pre-request, post-response)
- Plugin error handling and recovery
- Multiple plugin coordination
- Configuration validation

**Kill Switch:**
- Emergency mode activation and recovery
- Degraded mode partial functionality
- Request rejection during emergency
- State persistence across restarts

**Discord Integration:**
- Message reception and conversion to submissions
- Event-to-Discord message formatting
- Thread conversation tracking
- Command parsing and execution

Integration tests verify end-to-end scenarios with mock agents and real database connections.