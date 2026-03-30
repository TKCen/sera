# ADR-004: Permission Grant Persistence

**Status:** Proposed
**Date:** 2026-03-30
**Related:** Epic 3 (Sandbox), Story 3.10

## Context

When an agent requests access to a filesystem path outside its workspace, the operator approves the request and a grant is created. Currently, grants are stored in-memory in `PermissionRequestService` and lost on sera-core restart.

## Current State

- `PermissionRequestService` stores grants in `Map<string, PermissionGrant>`
- Three grant types: `session` (current session), `one-time` (single use), `persistent` (survives restarts)
- Despite the `persistent` type, ALL grants are in-memory only
- `AgentRegistry.getActiveFilesystemGrants()` queries the `agent_grants` table for bind-mount grants — these DO persist in PostgreSQL but are separate from permission request grants

## Problem

1. Operator approves filesystem access for an agent
2. Core restarts (deploy, crash, etc.)
3. Grant is lost — agent loses access, operator must re-approve
4. For persistent grants that were used for container bind mounts, the container was already created with the mount — but the permission record is gone

## Canonical Design (Story 3.10)

Persistent grants should be stored in PostgreSQL:
- `agent_grants` table already exists for bind-mount grants
- Permission request grants should use the same table or a sibling table
- Session grants remain in-memory (they're ephemeral by design)
- One-time grants can remain in-memory (consumed immediately)

## Recommended Approach

1. Add `permission_grants` table (or reuse `agent_grants` with a `grant_source` column)
2. When a `persistent` grant is approved, write to PostgreSQL
3. On startup, `PermissionRequestService` hydrates persistent grants from the DB
4. Session/one-time grants stay in-memory
5. Expired grants cleaned up by background job

## Schema

```sql
CREATE TABLE permission_grants (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  agent_instance_id uuid REFERENCES agent_instances(id),
  grant_type text NOT NULL CHECK (grant_type IN ('session', 'one-time', 'persistent')),
  resource_type text NOT NULL, -- 'filesystem', 'network', etc.
  resource_value text NOT NULL, -- path, hostname, etc.
  mode text DEFAULT 'ro', -- 'ro' or 'rw'
  approved_by text, -- operator ID
  created_at timestamptz DEFAULT now(),
  expires_at timestamptz,
  revoked_at timestamptz
);
```

## Consequences

### Positive
- Persistent grants survive restarts
- Audit trail of all grant approvals
- Operators don't need to re-approve after deploys

### Negative
- DB migration required
- Hydration adds startup latency (~50ms for typical grant counts)
- Need to handle stale grants (containers recreated with different IPs)
