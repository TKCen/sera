# Technical Audit Report: Interface & Communication Layer

## 1. Communication Protocol Analysis (Centrifugo/WebSockets)

The SERA platform utilizes **Centrifugo** as its real-time messaging backbone, employing a pub/sub architecture over WebSockets to facilitate low-latency communication between the `core` runtime and various client interfaces (`web`, `tui`).

### Architecture Overview
- **Protocol**: WebSocket (via Centrifugo) for real-time streams; HTTP REST API for command/control and history retrieval.
- **Message Envelope**: All messages follow a structured `IntercomMessage` schema, ensuring consistency across different consumers. This includes metadata such as `securityTier`, `replyTo`, and `ttl`.
- **Channel Namespacing**: A strict, versioned (v1) namespacing convention is enforced via `ChannelNamespace`. Patterns include:
  - `thoughts:{agentId}`: For streaming agent reasoning steps.
  - `tokens:{agentId}`: For high-frequency LLM token deltas.
  - `agent:{agentId}:status`: For lifecycle transitions (e.g., connecting, running, error).
  - `private:{sender}:{receiver}`: For direct agent-to-agent or agent-to-user messaging.
  - `circle:{circleId}`: For group broadcasts within a specific organizational scope.
  - `system.{event}`: For platform-wide notifications.

### Security & Authentication
- **Token-Based Access**: The system uses JWTs for both connection and subscription authorization. 
- **Role-Based Access Control (RBAC)**: The `IntercomService` implements granular permission checks. For example, the `viewer` role is restricted to subscribing only to `thoughts` channels, preventing unauthorized access to sensitive `private` or `token` streams.
*Note: Subscription tokens are generated server-side with specific claims (`sub`, `channel`, `role`) and short expiration windows (1h).*

## 2. React Dashboard ('web/') Subscription Mapping

The React dashboard acts as a primary observer of the agent ecosystem, subscribing to specific channels to provide real-time observability.

### Implementation Details
The dashboard uses a custom `CentrifugoProvider` wrapping the Centrifuge JS client. Key hooks drive the UI:

| Hook | Channel Pattern | Purpose |
| :--- | :--- | :--- |
| `useThoughtStream(agentId)` | `thoughts:{agentId}` | Populates the reasoning/log view with real-time agent "thoughts". |
| `useAgentStatus(agentId)` | `agent:{agentId}:status` | Updates UI indicators for agent health and lifecycle. |
| `useChannel<T>(channelName)` | Arbitrary | Generic hook for subscribing to any valid namespace (e.g., `tokens`, `circle`). |

### Data Flow
1. **Event Trigger**: `core/agent-runtime` executes a tool or processes an LLM response.
2. **Publication**: `IntercomService.publishThought()` is called in the backend.
3. **Relay**: Centrifugo receives the publication and broadcasts it via WebSocket to all active subscribers.
4. **UI Update**: The `useChannel` hook's `publication` listener triggers a React state update, re-rendering the relevant component (e.'g., Chat window or Agent list).

## 3. Go TUI ('tui/') Architecture Analysis

*Note: Based on current codebase investigation, the TUI is transitioning from a Go implementation to a Rust-based `sera-tui` crate.*

### Current/Emerging Architecture
The TUI architecture (as seen in the `rust/crates/sera-tui` structure) follows a modular view-based pattern:
- **Core Engine**: Uses `ratatui` for terminal UI rendering.
- **View Modules**: Segregated into specialized modules (`agents.rs`, `logs.rs`, `agent_detail.rs`) to manage different levels of information density.
- **Interaction Model**: The TUI acts as a high-privilege observer, likely subscribing to broader channel sets (e.g., `circle:*` or `system.*`) compared to the web dashboard.
- **Backend Interaction**: Communicates with `sera-core` via HTTP/REST for command execution and Centrifugo for real-time event polling/subscription.

## 4. Synchronization & Latency Identification

### Potential Issues

1. **Database vs. WebSocket Race Condition**:
   In `IntercomService.publishThought`, the service persists thoughts to PostgreSQL *before* publishing to Centrifugo. While this ensures data integrity, a high-frequency burst of events could lead to a "stale read" if a client attempts to fetch history via REST immediately after receiving a WebSocket event but before the DB transaction has fully committed/replicated (though unlikely in a single-node setup).

2. **Token Expiration Latency**:
   The `CentrifugoProvider` in the web client uses a `REFRESH_ADVANCE_MS` (60s) buffer to refresh JWTs. If network latency is high or the client is under heavy load, there is a window where the token might expire before the refresh completes, causing temporary disconnection/reconnection loops.

3. **UI State Desynchronization**:
   The `useChannel` hook in React replaces subscriptions on every `channelName` change. In rapid-fire scenarios (e.g., switching between many agents quickly), there is a risk of "ghost" events from previous subscriptions if the `unsubscribe()` cleanup does not execute before the new subscription begins processing.

4. **Token Stream Throughput**:
   The `publishToken` method sends individual WebSocket messages for every token delta. For high-throughput LLM models, this can saturate the WebSocket buffer and increase CPU overhead on the client-side React reconciliation process, potentially leading to UI "stuttering" during long generations.

## Conclusion
The Interface & Communication Layer is robustly designed with a clear separation of concerns and strong security primitives. The use of Centrifugo provides a scalable pub/sub foundation, though care must be taken regarding the frequency of token-level updates and the management of subscription lifecycles in high-density UI environments.
