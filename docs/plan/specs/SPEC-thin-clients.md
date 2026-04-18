# SPEC: Thin Clients & HMIs

> **Status:** DRAFT  
> **Source:** PRD §4.4 (thin clients), §7 (AG-UI minimal stream)  
> **Crate:** Consumed from `sera-agui` (minimal stream)  
> **Priority:** Low (Phase 3+) — architectural consideration now, implementation later  

---

## 1. Overview

Thin clients are **embedded, minimal displays** that consume a stripped-down AG-UI event stream. They are designed for use cases where a full SPA or CLI is impractical:

- Factory floor HMIs (Human-Machine Interfaces)
- Kiosks
- Embedded displays
- Mobile companion views
- Dashboard panels

Thin clients are **not** full SERA clients. They are consumers of a **minimal event stream** that any HTTP/SSE-capable device can process.

---

## 2. Design Principle

The AG-UI adapter (`sera-agui`) should expose two event stream tiers:

| Tier | Consumer | Events | Transport |
|---|---|---|---|
| **Full** | `sera-web`, rich AG-UI clients | All AG-UI events (streaming tokens, tool calls, metadata, etc.) | WebSocket / SSE |
| **Minimal** | Thin clients, HMIs, embedded | Streaming text, approval prompts, status updates | SSE / HTTP |

The architectural contract accommodates thin clients from day one, even if implementation ships later.

---

## 3. Minimal Event Set

> [!IMPORTANT]  
> **Requires further research.** The exact minimal event set for thin clients needs to be defined. Candidate events:

| Event | Description |
|---|---|
| `text_stream` | Streaming text content (token-by-token or chunked) |
| `text_complete` | Final complete text response |
| `approval_prompt` | Approval request requiring user action |
| `approval_result` | Result of an approval decision |
| `status_update` | Agent status change (idle, thinking, executing tool, waiting) |
| `error` | Error notification |
| `heartbeat` | Connection keep-alive |

### What's Excluded from Minimal

- Tool call details (arguments, schemas)
- Context assembly metadata
- Memory operations
- Session lifecycle details
- Configuration events

---

## 4. Transport

Thin clients use **SSE (Server-Sent Events)** or **plain HTTP streaming** — no WebSocket required. This maximizes compatibility with embedded devices and simple HTTP clients.

```
GET /api/v1/agents/{agent}/stream?tier=minimal
Accept: text/event-stream

data: {"type":"status_update","status":"thinking","agent":"sera"}
data: {"type":"text_stream","content":"I'll look into that"}
data: {"type":"text_stream","content":" for you..."}
data: {"type":"text_complete","content":"I'll look into that for you..."}
data: {"type":"approval_prompt","id":"abc123","description":"Delete file X?","urgency":"medium"}
```

---

## 5. Approval on Thin Clients

Thin clients can display approval prompts and submit approval decisions:

```
POST /api/v1/approvals/{id}/decide
Content-Type: application/json

{"decision": "approve"}
```

This enables factory floor operators to approve agent actions from an HMI panel.

---

## 6. Authentication

Thin clients authenticate via **API keys** or **JWT tokens** — same as other clients. The gateway enforces authorization for the connected principal.

---

## 7. Configuration

```yaml
sera:
  interop:
    agui:
      minimal_stream:
        enabled: true
        # Exposed on the gateway HTTP port
        path: "/api/v1/agents/{agent}/stream"
```

---

## 8. Cross-References

| Dependency | Spec | Relationship |
|---|---|---|
| `sera-agui` | [SPEC-interop](SPEC-interop.md) | Minimal stream served by AG-UI adapter |
| `sera-gateway` | [SPEC-gateway](SPEC-gateway.md) | Stream endpoint on gateway HTTP |
| `sera-auth` | [SPEC-identity-authz](SPEC-identity-authz.md) | Thin client authentication |
| `sera-hitl` | [SPEC-hitl-approval](SPEC-hitl-approval.md) | Approval prompts and decisions |

---

## 9. Open Questions

1. **Minimum AG-UI event set** — What events are strictly necessary for HMI use cases? (see §3)
2. **Push notifications** — Should thin clients support push notifications (e.g., webhook callback) for devices that can't maintain SSE connections?
3. **Display constraints** — Should the minimal stream include formatting hints for small displays (character limits, importance levels)?
4. **Bidirectional input** — Can thin clients send messages to agents, or are they read-only + approval-only?
5. **Discovery** — How does a thin client discover available agents and their stream endpoints?
