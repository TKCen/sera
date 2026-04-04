# SERA API Route Inventory

Extracted from `core/src/routes/*.ts` and `core/src/index.ts`.

Stability: **stable** = unchanged 3+ months | **changing** = actively developed | **legacy** = deprecated

## Health (Public)

| Method | Path                 | Auth   | Stability |
| ------ | -------------------- | ------ | --------- |
| GET    | `/api/health`        | Public | stable    |
| GET    | `/api/health/detail` | Public | stable    |

## Authentication

| Method | Path                      | Auth      | Stability |
| ------ | ------------------------- | --------- | --------- |
| POST   | `/api/auth/login`         | Public    | stable    |
| POST   | `/api/auth/logout`        | Protected | stable    |
| GET    | `/api/auth/session`       | Protected | stable    |
| POST   | `/api/auth/token/refresh` | Protected | stable    |
| GET    | `/api/auth/oidc/callback` | Public    | stable    |

## Agents

| Method | Path                          | Auth      | Stability |
| ------ | ----------------------------- | --------- | --------- |
| GET    | `/api/agents`                 | Protected | stable    |
| GET    | `/api/agents/instances`       | Protected | stable    |
| POST   | `/api/agents/instances`       | Protected | stable    |
| GET    | `/api/agents/instances/:id`   | Protected | stable    |
| PATCH  | `/api/agents/instances/:id`   | Protected | stable    |
| DELETE | `/api/agents/instances/:id`   | Protected | stable    |
| GET    | `/api/agents/templates`       | Protected | stable    |
| GET    | `/api/agents/templates/:name` | Protected | stable    |
| POST   | `/api/agents/start/:id`       | Protected | stable    |
| POST   | `/api/agents/stop/:id`        | Protected | stable    |

## LLM Proxy

| Method | Path                       | Auth      | Stability |
| ------ | -------------------------- | --------- | --------- |
| POST   | `/v1/llm/chat/completions` | Protected | stable    |
| POST   | `/v1/chat/completions`     | Protected | stable    |

## Chat

| Method | Path        | Auth      | Stability |
| ------ | ----------- | --------- | --------- |
| POST   | `/api/chat` | Protected | stable    |

## Providers

| Method | Path                  | Auth      | Stability |
| ------ | --------------------- | --------- | --------- |
| GET    | `/api/providers/list` | Protected | stable    |
| POST   | `/api/providers`      | Protected | stable    |
| DELETE | `/api/providers/:id`  | Protected | stable    |

## Budget & Metering

| Method | Path                            | Auth      | Stability |
| ------ | ------------------------------- | --------- | --------- |
| GET    | `/api/budget`                   | Protected | stable    |
| GET    | `/api/budget/agents/:id/budget` | Protected | stable    |
| PATCH  | `/api/budget/agents/:id/budget` | Protected | stable    |
| GET    | `/api/metering/usage`           | Protected | stable    |

## Audit

| Method | Path                | Auth      | Stability |
| ------ | ------------------- | --------- | --------- |
| GET    | `/api/audit/log`    | Protected | stable    |
| GET    | `/api/audit/verify` | Protected | stable    |
| GET    | `/api/audit/export` | Protected | stable    |

## Memory & Knowledge

| Method | Path                                      | Auth      | Stability |
| ------ | ----------------------------------------- | --------- | --------- |
| GET    | `/api/memory/blocks/:instanceId`          | Protected | stable    |
| POST   | `/api/memory/blocks/:instanceId`          | Protected | stable    |
| PATCH  | `/api/memory/blocks/:instanceId/:blockId` | Protected | stable    |
| DELETE | `/api/memory/blocks/:instanceId/:blockId` | Protected | stable    |
| POST   | `/api/knowledge/query`                    | Protected | stable    |
| POST   | `/api/knowledge/store`                    | Protected | stable    |

## Skills & Tools

| Method | Path                | Auth      | Stability |
| ------ | ------------------- | --------- | --------- |
| GET    | `/api/skills`       | Protected | stable    |
| GET    | `/api/skills/:id`   | Protected | stable    |
| POST   | `/api/skills`       | Protected | stable    |
| PUT    | `/api/skills/:id`   | Protected | stable    |
| DELETE | `/api/skills/:id`   | Protected | stable    |
| POST   | `/v1/tools/:toolId` | Protected | stable    |

## Sandbox

| Method | Path                          | Auth      | Stability |
| ------ | ----------------------------- | --------- | --------- |
| POST   | `/api/sandbox/spawn`          | Protected | stable    |
| POST   | `/api/sandbox/exec`           | Protected | stable    |
| GET    | `/api/sandbox/containers`     | Protected | stable    |
| DELETE | `/api/sandbox/containers/:id` | Protected | stable    |

## Schedules

| Method | Path                         | Auth      | Stability |
| ------ | ---------------------------- | --------- | --------- |
| GET    | `/api/schedules`             | Protected | stable    |
| POST   | `/api/schedules`             | Protected | stable    |
| DELETE | `/api/schedules/:id`         | Protected | stable    |
| POST   | `/api/schedules/:id/trigger` | Protected | stable    |

## Sessions

| Method | Path                | Auth      | Stability |
| ------ | ------------------- | --------- | --------- |
| GET    | `/api/sessions`     | Protected | stable    |
| GET    | `/api/sessions/:id` | Protected | stable    |
| DELETE | `/api/sessions/:id` | Protected | stable    |

## Circles

| Method | Path               | Auth      | Stability |
| ------ | ------------------ | --------- | --------- |
| GET    | `/api/circles`     | Protected | stable    |
| POST   | `/api/circles`     | Protected | changing  |
| GET    | `/api/circles/:id` | Protected | stable    |

## Intercom

| Method | Path                    | Auth      | Stability |
| ------ | ----------------------- | --------- | --------- |
| POST   | `/api/intercom/publish` | Protected | stable    |
| POST   | `/api/intercom/token`   | Protected | stable    |

## Notifications & Channels

| Method | Path                              | Auth      | Stability |
| ------ | --------------------------------- | --------- | --------- |
| GET    | `/api/notifications/channels`     | Protected | stable    |
| POST   | `/api/notifications/channels`     | Protected | stable    |
| DELETE | `/api/notifications/channels/:id` | Protected | stable    |
| POST   | `/api/notifications/test/:id`     | Protected | stable    |

## Secrets

| Method | Path                | Auth      | Stability |
| ------ | ------------------- | --------- | --------- |
| GET    | `/api/secrets`      | Protected | stable    |
| POST   | `/api/secrets`      | Protected | stable    |
| DELETE | `/api/secrets/:key` | Protected | stable    |

## MCP Servers

| Method | Path                   | Auth      | Stability |
| ------ | ---------------------- | --------- | --------- |
| GET    | `/api/mcp-servers`     | Protected | stable    |
| POST   | `/api/mcp-servers`     | Protected | stable    |
| DELETE | `/api/mcp-servers/:id` | Protected | stable    |

## Operator Requests

| Method | Path                                 | Auth      | Stability |
| ------ | ------------------------------------ | --------- | --------- |
| GET    | `/api/operator-requests`             | Protected | stable    |
| POST   | `/api/operator-requests/:id/respond` | Protected | stable    |

## Delegation

| Method | Path                       | Auth      | Stability |
| ------ | -------------------------- | --------- | --------- |
| POST   | `/api/delegation/delegate` | Protected | stable    |

## Registry

| Method | Path                   | Auth      | Stability |
| ------ | ---------------------- | --------- | --------- |
| POST   | `/api/registry/import` | Protected | stable    |

## Config

| Method | Path          | Auth      | Stability |
| ------ | ------------- | --------- | --------- |
| GET    | `/api/config` | Protected | stable    |

## Embedding

| Method | Path             | Auth      | Stability |
| ------ | ---------------- | --------- | --------- |
| POST   | `/api/embedding` | Protected | stable    |

## Heartbeat

| Method | Path                    | Auth      | Stability |
| ------ | ----------------------- | --------- | --------- |
| POST   | `/api/agents/heartbeat` | Protected | stable    |

## Pipelines

| Method | Path             | Auth      | Stability |
| ------ | ---------------- | --------- | --------- |
| POST   | `/api/pipelines` | Protected | changing  |

## Federation

| Method | Path              | Auth      | Stability |
| ------ | ----------------- | --------- | --------- |
| GET    | `/api/federation` | Protected | changing  |
| POST   | `/api/federation` | Protected | changing  |
