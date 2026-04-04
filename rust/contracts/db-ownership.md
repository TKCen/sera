# SERA Database Table Ownership

Each table has exactly one writer during migration. Read-sharing is allowed.

## Agent Management

| Table                      | Write Owner                 | Readers                      | Key Columns                                                                               |
| -------------------------- | --------------------------- | ---------------------------- | ----------------------------------------------------------------------------------------- |
| `agent_templates`          | AgentRegistry               | Orchestrator, API routes     | name (PK), display_name, builtin, category, spec (JSONB)                                  |
| `agent_instances`          | AgentRegistry, Orchestrator | API routes, HeartbeatService | id (PK), name, template_ref (FK), status, lifecycle_mode, container_id, last_heartbeat_at |
| `agent_instance_overrides` | AgentRegistry               | Orchestrator                 | agent_instance_id (FK), key, value                                                        |

## Metering & Budgeting

| Table          | Write Owner                    | Readers                     | Key Columns                                                                            |
| -------------- | ------------------------------ | --------------------------- | -------------------------------------------------------------------------------------- |
| `token_usage`  | MeteringService                | API routes (metering)       | agent_id, circle_id, model, prompt_tokens, completion_tokens, total_tokens, created_at |
| `usage_events` | MeteringService                | API routes (metering)       | agent_id, model, prompt_tokens, completion_tokens, cost_usd, latency_ms, status        |
| `token_quotas` | MeteringService, AgentRegistry | MeteringService.checkBudget | agent_id (PK), max_tokens_per_hour, max_tokens_per_day                                 |

## Memory & Knowledge

| Table                  | Write Owner            | Readers       | Key Columns                                                                   |
| ---------------------- | ---------------------- | ------------- | ----------------------------------------------------------------------------- |
| `core_memory_blocks`   | CoreMemoryService      | API routes    | id (PK), agent_instance_id (FK), name, content, character_limit, is_read_only |
| `memory_blocks`        | MemoryBlockStore       | MemoryManager | id (PK), scope, key, value (JSONB)                                            |
| `scoped_memory_blocks` | ScopedMemoryBlockStore | MemoryManager | id (PK), agent_instance_id (FK), scope, key, value (JSONB)                    |

## Audit & Compliance

| Table         | Write Owner  | Readers            | Key Columns                                                                                  |
| ------------- | ------------ | ------------------ | -------------------------------------------------------------------------------------------- |
| `audit_trail` | AuditService | API routes (audit) | sequence (PK), timestamp, actor_type, actor_id, event_type, payload (JSONB), prev_hash, hash |

## Permissions & Security

| Table                 | Write Owner              | Readers                  | Key Columns                                                 |
| --------------------- | ------------------------ | ------------------------ | ----------------------------------------------------------- |
| `permission_requests` | PermissionRequestService | API routes, Orchestrator | request_id (PK), agent_id, dimension, value, reason, status |
| `persistent_grants`   | PermissionRequestService | SandboxManager           | id (PK), agent_instance_id (FK), dimension, value           |

## Secrets

| Table     | Write Owner    | Readers          | Key Columns                           |
| --------- | -------------- | ---------------- | ------------------------------------- |
| `secrets` | SecretsManager | ProviderRegistry | key (PK), encrypted_value, created_at |

## Skills

| Table           | Write Owner  | Readers                   | Key Columns                                        |
| --------------- | ------------ | ------------------------- | -------------------------------------------------- |
| `skill_library` | SkillLibrary | SkillRegistry, API routes | id (PK), name, version, definition (JSONB), source |

## Notifications

| Table                   | Write Owner         | Readers    | Key Columns                                  |
| ----------------------- | ------------------- | ---------- | -------------------------------------------- |
| `notification_channels` | NotificationService | API routes | id (PK), name, type, config (JSONB), enabled |

## Operators & Auth

| Table       | Write Owner           | Readers                        | Key Columns                                            |
| ----------- | --------------------- | ------------------------------ | ------------------------------------------------------ |
| `operators` | AuthService (via API) | ApiKeyProvider, AuthMiddleware | id (PK), email (unique), name, api_keys (JSONB), roles |

## Sessions

| Table      | Write Owner  | Readers    | Key Columns                                                               |
| ---------- | ------------ | ---------- | ------------------------------------------------------------------------- |
| `sessions` | SessionStore | API routes | id (PK), agent_instance_id (FK), metadata (JSONB), created_at, expires_at |

## Schedules

| Table       | Write Owner     | Readers    | Key Columns                                                                           |
| ----------- | --------------- | ---------- | ------------------------------------------------------------------------------------- |
| `schedules` | ScheduleService | API routes | id (PK), agent_instance_id, name, type, expression, task (JSONB), status, last_run_at |

## pg-boss Internal

| Table             | Write Owner     | Readers             | Key Columns                       |
| ----------------- | --------------- | ------------------- | --------------------------------- |
| `pgboss.job`      | PgBossService   | All queue consumers | id, name, state, data, created_on |
| `pgboss.schedule` | PgBossService   | ScheduleService     | name, cron, data                  |
| `pgmigrations`    | node-pg-migrate | initDb()            | id, name, run_on                  |

## Migration Rules

During the strangler fig migration:

1. **One writer per table at any time** — no dual writes without idempotency keys
2. **Read-sharing is always allowed** — both TS and Rust can read any table
3. **Schema changes must be additive** — no destructive renames until old writer is removed
4. **Ownership transfers subsystem by subsystem** — per Phase 4 ordering in MIGRATION-PLAN.md
