# Epic 27: Interactive Setup & Diagnostics

## Overview

SERA's current setup experience requires reading documentation, manually editing YAML/JSON configs, and debugging opaque Docker errors. OpenClaw's `doctor` command, setup wizard, and guided channel configuration flows dramatically lower the barrier to entry. This epic brings the same UX to SERA — a `sera doctor` diagnostic command, an interactive setup wizard for first-time configuration, and guided flows for adding LLM providers and channels.

## Context

- **Reference implementation:** OpenClaw `src/commands/doctor/`, `src/wizard/`, `src/flows/`, `src/commands/setup/`
- **OpenClaw's approach:** CLI-driven doctor command checks config, credentials, connectivity. Setup wizard walks through provider selection, model picking, and channel configuration interactively. Provider flow validates API keys before saving.
- **SERA's advantage:** SERA's Docker-based architecture means `sera doctor` can also check container health, network connectivity, volume state, and service dependencies — more comprehensive than OpenClaw's process-level checks.
- **Existing issues:** #150 (sera doctor), partially tracked but no epic spec

## Dependencies

- Epic 01 (Infrastructure) — Docker Compose services to check
- Epic 04 (LLM Proxy) — provider configuration to validate
- Epic 15 (Plugin SDK) — `sera` CLI foundation
- Epic 16 (Auth & Secrets) — secrets validation

---

## Stories

### Story 27.1: `sera doctor` diagnostic command

**As** an operator
**I want** a single command that diagnoses common SERA configuration and runtime problems
**So that** I can quickly identify and fix issues without reading logs manually

**Acceptance Criteria:**

- [ ] `sera doctor` CLI command (runs without a running sera-core for config checks; connects for runtime checks)
- [ ] **Configuration checks** (offline — no running services needed):
  - [ ] Docker Compose files valid and all required services defined
  - [ ] `docker-compose.dev.yaml` overlay exists and references correct base
  - [ ] Agent manifests in `agents/` valid against schema
  - [ ] `core/config/providers.json` valid — at least one provider configured
  - [ ] `.env` or `docker-compose.yaml` has required env vars (`POSTGRES_PASSWORD`, `SERA_API_KEY`)
  - [ ] Shell scripts (`.sh`) have LF line endings (Windows gotcha)
  - [ ] `bun.lock` files exist and are not stale (compare `package.json` mtime)
- [ ] **Runtime checks** (requires running services):
  - [ ] Docker daemon reachable
  - [ ] All Docker Compose services running and healthy
  - [ ] PostgreSQL connection successful, migrations up to date
  - [ ] Qdrant connection successful, collections exist
  - [ ] Centrifugo healthy and WebSocket connectable
  - [ ] Egress proxy (Squid) healthy (if configured)
  - [ ] At least one LLM provider reachable (test request with tiny prompt)
  - [ ] Embedding service available (test embedding generation)
  - [ ] Agent worker image (`sera-agent-worker:latest`) exists and up to date
  - [ ] `sera_net` and `agent_net` Docker networks exist
  - [ ] Named volumes (`node_modules_core`, `node_modules_web`) populated
- [ ] **Credential checks:**
  - [ ] API keys in secrets store are valid (test with provider health endpoint)
  - [ ] Discord bot tokens valid (if Discord channel configured)
  - [ ] SMTP credentials valid (if email channel configured)
- [ ] **Output format:**
  - Each check: `[PASS]`, `[WARN]`, or `[FAIL]` with description and fix suggestion
  - Summary at end: total pass/warn/fail counts
  - `--json` flag for machine-readable output
  - `--fix` flag: auto-fix common issues (set LF line endings, regenerate lockfiles, create missing directories)
- [ ] **Exit codes:** 0 = all pass, 1 = warnings only, 2 = failures exist

---

### Story 27.2: First-run setup wizard

**As** a new SERA operator
**I want** an interactive setup wizard that guides me through initial configuration
**So that** I can get SERA running without reading multiple docs pages

**Acceptance Criteria:**

- [ ] `sera setup` CLI command launches the interactive wizard
- [ ] Auto-detects first run: no `.env` file, no `providers.json` entries, no secrets
- [ ] **Step 1 — Environment:**
  - Check Docker installed and running
  - Check `docker compose` available (v2)
  - Check bun installed and version compatible
  - Auto-create `.env` from `.env.example` with prompted values
- [ ] **Step 2 — Database:**
  - Generate secure `POSTGRES_PASSWORD` (random 32-char)
  - Offer: use bundled PostgreSQL (default) or external connection string
  - Test connection if external
- [ ] **Step 3 — LLM Provider:**
  - Interactive model picker: "Which LLM providers do you use?"
  - Options: LM Studio (local), Ollama (local), OpenAI, Anthropic, Google, Custom
  - For each selected: prompt for API key or base URL
  - Test connection with a minimal completion request
  - Write to `core/config/providers.json`
- [ ] **Step 4 — Embedding Model:**
  - Options: Local (LM Studio/Ollama with embedding model), OpenAI, HuggingFace
  - Configure `core/config/embedding.json`
  - Test embedding generation
- [ ] **Step 5 — Default Agent:**
  - Create a default "Sera" agent from template
  - Configure model name to match selected provider
  - Write `agents/sera.yaml`
- [ ] **Step 6 — Launch:**
  - Run `docker compose up -d` with appropriate overlay
  - Wait for health checks
  - Print access URL and API key
  - Offer: "Add a Discord bot? Add Telegram? (y/n)"
- [ ] Wizard state persisted to `.sera-setup.json` so it can be resumed if interrupted
- [ ] Non-interactive mode: `sera setup --non-interactive --provider openai --api-key sk-...`

---

### Story 27.3: Provider setup flow

**As** an operator
**I want** a guided flow for adding or changing LLM providers
**So that** I can configure new models without hand-editing JSON

**Acceptance Criteria:**

- [ ] `sera providers add` CLI command with interactive flow
- [ ] Provider type selection: local (LM Studio, Ollama) or cloud (OpenAI, Anthropic, Google, custom)
- [ ] **Local provider flow:**
  - Prompt for base URL (with auto-detection: scan common ports 1234, 11434, 8080)
  - List available models from the local server's `/v1/models` endpoint
  - Select models to register
  - Auto-detect capabilities (context window, vision, reasoning) from model metadata if available
- [ ] **Cloud provider flow:**
  - Prompt for API key
  - Validate key with a test request
  - Store key in secrets store (never in `providers.json` as plaintext)
  - List available models or accept user-specified model names
- [ ] **Thinking/reasoning model detection:**
  - Detect models that support thinking/reasoning (name pattern matching + test)
  - Set `reasoning: true` automatically
  - Configure thinking level: low / medium / high / x-high
- [ ] Write provider config to `core/config/providers.json`
- [ ] `sera providers list` — show configured providers with connection status
- [ ] `sera providers test <id>` — test a specific provider with a completion request
- [ ] `sera providers remove <id>` — remove a provider (warns if agents reference it)

---

### Story 27.4: Channel setup flow

**As** an operator
**I want** a guided flow for adding messaging channels
**So that** I can connect Discord/Telegram/Slack bots without manual configuration

**Acceptance Criteria:**

- [ ] `sera channels add` CLI command with interactive flow
- [ ] Channel type selection: Discord, Telegram, Slack, WhatsApp, Email, Webhook
- [ ] **Per-channel guided setup:**
  - **Discord:** Links to Developer Portal, prompts for bot token, validates token, prompts for guild ID (lists guilds bot is in), selects binding mode, creates channel
  - **Telegram:** Links to @BotFather, prompts for bot token, validates via `getMe`, selects binding mode, creates channel
  - **Slack:** Links to api.slack.com, prompts for app token and signing secret, validates, creates channel
  - **Email:** Prompts for SMTP config, tests send with a test email, creates channel
  - **Webhook:** Generates webhook URL and secret, displays for user to configure externally
- [ ] Each flow includes prerequisite steps with links and explanations
- [ ] Binding mode selection: "What should this channel connect to?" → agent (pick agent) / circle (pick circle) / notifications only
- [ ] Test message sent through channel on completion: "SERA is connected!"
- [ ] `sera channels list` — show configured channels with status
- [ ] `sera channels test <id>` — send a test message
- [ ] `sera channels remove <id>` — remove a channel

---

### Story 27.5: Health dashboard in sera-web

**As** an operator using the web dashboard
**I want** a system health page showing the same information as `sera doctor`
**So that** I can monitor system health without SSH access

**Acceptance Criteria:**

- [ ] `/settings/health` page in sera-web
- [ ] **Service status cards:** each Docker service (core, web, postgres, qdrant, centrifugo, egress-proxy) with health badge
- [ ] **Provider status:** each LLM provider with last successful request time, latency, error rate
- [ ] **Connectivity matrix:** which services can reach which others (core → postgres, core → qdrant, etc.)
- [ ] **Disk/volume status:** named volumes with used/available space
- [ ] **Recent errors:** last 10 errors from each service (from Docker logs)
- [ ] Auto-refresh every 30s
- [ ] `GET /api/health/detailed` API endpoint powering this page (extends existing `/api/health`)
- [ ] Color-coded: green (healthy), amber (degraded), red (down)

---

### Story 27.6: Onboarding checklist in sera-web

**As** a new operator opening sera-web for the first time
**I want** an onboarding checklist guiding me through essential setup steps
**So that** I know what to configure and in what order

**Acceptance Criteria:**

- [ ] Onboarding overlay shown when:
  - No agents configured (besides defaults)
  - No channels configured
  - No operator OIDC configured (using bootstrap API key)
- [ ] Checklist items:
  1. "Configure an LLM provider" → links to Providers page
  2. "Create your first agent" → links to Agent create page
  3. "Chat with your agent" → links to Chat page
  4. "Connect a messaging channel" → links to Channels page
  5. "Set up authentication" → links to Settings page
- [ ] Each item shows completion status (checked/unchecked based on current state)
- [ ] "Don't show again" dismiss option (persisted in `operator_preferences`)
- [ ] Compact banner version shown on dashboard after first dismiss (can be permanently hidden)

---

## Technical Notes

- The `sera` CLI commands in this epic extend the CLI foundation from Epic 15 (Story 15.3)
- All secrets prompting uses masked input (no echo to terminal)
- Interactive prompts use `@inquirer/prompts` for consistent cross-platform UX
- The wizard and setup flows work on Windows (Git Bash), macOS, and Linux
- Provider auto-detection (scanning ports) respects timeouts (500ms per port) to avoid blocking
