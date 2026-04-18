# SERA Channel Namespaces (v1)

This document defines the canonical channel names used for real-time messaging in SERA. 
These names are a stable contract — all components must use these exact patterns.

## Canonical Channels

| Pattern | Description | Namespace | History |
|---|---|---|---|
| `thoughts:{agentId}` | Agent thought/reasoning stream | `thoughts` | 100 msgs, 1h TTL |
| `tokens:{agentId}` | Streaming LLM token deltas | `tokens` | None (stream only) |
| `agent:{agentId}:status` | Lifecycle status (started, stopped, error) | `agent` | 10 msgs, 10m TTL |
| `private:{agentId}:{targetId}` | Direct agent-to-agent messaging | `private` | 100 msgs, 1h TTL |
| `circle:{circleId}` | Circle broadcast messages | `circle` | 50 msgs, 4h TTL |
| `system.{event}` | Platform events (system.agents, system.tools, etc.) | `system` | 20 msgs, 30m TTL |

## Constraints
- **UUIDs only**: `{agentId}` and `{circleId}` must be the unique UUID or slug, not the display name.
- **Lowercase**: All channel names and event types must be lowercase.
- **Segments**: Segments are separated by colons (`:`) or dots (`.`) as defined above.
