# Audit Trail

Every agent action in SERA produces an auditable event record stored in a Merkle hash-chain in PostgreSQL. This provides tamper-evident logging with cryptographic integrity verification.

## How It Works

Each audit event contains:

| Field           | Description                                                      |
| --------------- | ---------------------------------------------------------------- |
| `eventType`     | Action type (e.g., `tool.execute`, `llm.call`, `network.egress`) |
| `agentId`       | The agent that performed the action                              |
| `actingContext` | Full delegation chain — who authorised this action               |
| `payload`       | Event-specific data (tool name, arguments, result summary)       |
| `timestamp`     | When the event occurred                                          |
| `hash`          | SHA-256 hash of this event + previous event's hash               |
| `previousHash`  | Hash of the preceding event in the chain                         |

## Merkle Hash Chain

Events are chained cryptographically:

```
Event 1: hash = SHA-256(event1_data + "genesis")
Event 2: hash = SHA-256(event2_data + event1.hash)
Event 3: hash = SHA-256(event3_data + event2.hash)
...
```

This means:

- **Tampering is detectable** — modifying any event breaks the chain
- **Completeness is verifiable** — gaps in the sequence are visible
- **Non-repudiation** — events carry the full acting context (who authorised what)

## Event Types

| Category            | Events                                                        |
| ------------------- | ------------------------------------------------------------- |
| **Agent lifecycle** | `agent.start`, `agent.stop`, `agent.create`, `agent.delete`   |
| **LLM**             | `llm.call`, `llm.budget.exceeded`, `llm.error`                |
| **Tools**           | `tool.execute`, `tool.result`, `tool.error`                   |
| **Memory**          | `knowledge.store`, `knowledge.query`, `knowledge.merge`       |
| **Network**         | `network.egress`, `network.egress.denied`                     |
| **Permissions**     | `permission.request`, `permission.grant`, `permission.deny`   |
| **Delegation**      | `delegation.request`, `delegation.grant`, `delegation.revoke` |
| **Scheduling**      | `schedule.create`, `schedule.trigger`, `schedule.complete`    |

## Viewing the Audit Trail

The web dashboard provides an **Audit Log** page with:

- Filterable event timeline
- Agent and event type filters
- Chain integrity verification
- Export functionality (CSV, JSON)

## API Access

```bash
# List audit events
GET /api/audit/events?agentId={id}&eventType={type}&from={date}&to={date}

# Verify chain integrity
GET /api/audit/verify?from={eventId}&to={eventId}

# Export events
GET /api/audit/export?format=csv&from={date}&to={date}
```
