# Federation Design (Story 9.6)

## Overview

SERA handles cross-instance communication via a "Bridge" architecture. Each SERA instance can act as both a publisher and subscriber to other instances. This enables agents in one instance to message agents in another instance, or participate in cross-instance circles.

## Channel Namespace

The `federation:` namespace is reserved for cross-instance coordination.

- `federation:{remoteInstanceId}` — used for heartbeats and peering handshakes.

## Peering Mechanism (Future Spec)

1. **Discovery**: Instances find each other via static configuration or a discovery service.
2. **Handshake**: Instances exchange identity tokens and verify mTLS certificates.
3. **Subscription**: Instance A subscribes to relevant channels on Instance B's Centrifugo (or proxies via `bridge:` routes).
4. **Routing**: The `BridgeService` on Instance A detects messages for agents on Instance B and forwards them via the bridge route.

## Implementation Status (v1)

In v1, federation is stubbed. The `BridgeService` exists to define the architectural boundary, but no actual cross-instance socket or HTTP connections are maintained beyond the internal `bridge:` routes for local container orchestration.

- `GET /api/federation/peers`: Returns `[]`.
- `BridgeService`: Methods `connect`, `disconnect`, and `route` are implemented as logging stubs.
