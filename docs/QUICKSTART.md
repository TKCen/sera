# Developer Quickstart

Get SERA running locally in under 5 minutes.

## Prerequisites

- **Docker Desktop** with WSL2 (Windows) or Docker Engine (Linux/macOS)
- **Bun** v1.1+ (`curl -fsSL https://bun.sh/install | bash`)
- **Git**
- **LM Studio** or **Ollama** for local LLM inference (optional — can use cloud providers)

## 1. Clone and Install

```bash
git clone https://github.com/TKCen/sera.git
cd sera
bun install
```

## 2. Configure Environment

```bash
cp .env.example .env
# Edit .env — set at minimum:
#   SERA_BOOTSTRAP_API_KEY=<any random string>
#   LLM_BASE_URL=http://host.docker.internal:1234/v1  # for LM Studio
```

For cloud providers (optional):
```bash
# Add to .env:
OPENAI_API_KEY=sk-...
# or
ANTHROPIC_API_KEY=sk-ant-...
# or
GOOGLE_API_KEY=AIza...
```

## 3. Start the Stack

```bash
docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up -d
```

This starts:
- **sera-core** (API server) — `http://localhost:3001`
- **sera-web** (dashboard) — `http://localhost:3000`
- **sera-db** (PostgreSQL)
- **centrifugo** (real-time messaging)
- **qdrant** (vector search)
- **sera-egress-proxy** (Squid outbound proxy)

Wait ~30 seconds for migrations and initial setup.

## 4. Verify

```bash
curl -s -H "Authorization: Bearer <your-api-key>" http://localhost:3001/api/health
# → {"status":"ok","service":"sera-core"}
```

Open `http://localhost:3000` in your browser for the dashboard.

## 5. Create Your First Agent

Via the web UI: Agents → Create → pick a template (e.g., "researcher") → Start.

Or via API:
```bash
curl -X POST http://localhost:3001/api/agents/instances \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"templateRef": "researcher", "name": "my-researcher", "start": true}'
```

## 6. Chat with an Agent

Via the web UI: Chat → select your agent → type a message.

Or via API:
```bash
curl -X POST http://localhost:3001/api/chat \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{"agentName": "my-researcher", "message": "What can you do?"}'
```

## Development Workflow

### Running Tests

```bash
bun run ci          # Full CI: format + typecheck + lint + test + build
bun run test        # Tests only (core + web)
```

### Making Changes

1. Edit source files in `core/src/` or `web/src/`
2. Restart the affected container: `docker compose restart sera-core` (or `sera-web`)
3. For agent-runtime changes: rebuild the image first:
   ```bash
   docker build -f core/sandbox/Dockerfile.worker -t sera-agent-worker:latest core/
   ```

### Code Quality

```bash
bun run format      # Auto-format all files
bun run typecheck   # TypeScript strict mode
bun run lint        # ESLint
```

## Project Structure

```
sera/
├── core/                  # sera-core API server (Node.js/Express)
│   ├── agent-runtime/     # Agent worker process (Bun)
│   ├── config/            # Runtime config (providers.json)
│   └── src/               # Source code
├── web/                   # sera-web dashboard (React/Vite)
├── docs/                  # Architecture, epics, ADRs
├── agents/                # Agent YAML manifests
├── templates/             # AgentTemplate definitions
├── schemas/               # JSON Schema for manifests
└── docker-compose.yaml    # Dev stack definition
```

## Next Steps

- Read [ARCHITECTURE.md](ARCHITECTURE.md) for the full system design
- Check [CLAUDE.md](../CLAUDE.md) for codebase conventions and learnings
- See [TESTING.md](TESTING.md) for testing patterns
- Browse [epics/](epics/) for feature specifications
