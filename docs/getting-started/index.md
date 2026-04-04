# Getting Started

This section walks you through setting up SERA from scratch.

## Prerequisites

| Requirement             | Version             | Notes                                                         |
| ----------------------- | ------------------- | ------------------------------------------------------------- |
| Docker Desktop / Engine | 24+ with Compose v2 | Required for all services                                     |
| Bun                     | 1.1+                | Package manager and script runner                             |
| Git                     | Any recent          | For cloning the repository                                    |
| LLM Provider            | Any                 | LM Studio, Ollama (local), or OpenAI/Anthropic/Google (cloud) |

## What gets deployed

When you run `bun run dev:up`, Docker Compose starts:

| Service               | Port            | Purpose                                 |
| --------------------- | --------------- | --------------------------------------- |
| **sera-core**         | 3001            | API server, orchestrator, LLM proxy     |
| **sera-web**          | 3000            | Operator dashboard (React SPA)          |
| **sera-db**           | 5432            | PostgreSQL 15 + pgvector                |
| **qdrant**            | 6333            | Vector database for semantic memory     |
| **centrifugo**        | 10001           | Real-time WebSocket pub/sub             |
| **sera-egress-proxy** | 3128 (internal) | Squid proxy for agent network filtering |

On first boot, sera-core automatically:

1. Runs database migrations
2. Imports sandbox boundaries, named lists, and templates from YAML files
3. Bootstraps the **Sera** agent (the primary resident agent)
4. Prints the bootstrap API key to the log

## Next steps

<div class="grid cards" markdown>

- [:octicons-rocket-24: **Quick Start**](../QUICKSTART.md)

  Step-by-step setup in under 5 minutes

- [:octicons-gear-24: **Configuration**](configuration.md)

  Environment variables, LLM providers, and authentication

- [:octicons-dependabot-24: **Your First Agent**](first-agent.md)

  Create and configure a custom agent from a template

- [:octicons-server-24: **Deployment**](../DEPLOYMENT.md)

  Production deployment with Docker Compose

</div>
