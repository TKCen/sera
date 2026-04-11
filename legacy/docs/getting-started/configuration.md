# Configuration

SERA is configured through environment variables (`.env` file) and YAML manifest files.

## Environment Variables

Copy `.env.example` to `.env` and configure the required settings.

### Required

| Variable                 | Description                                          | Example                                                 |
| ------------------------ | ---------------------------------------------------- | ------------------------------------------------------- |
| `SERA_BOOTSTRAP_API_KEY` | API key for operator authentication                  | `sera_prod_abc123...`                                   |
| `SECRETS_MASTER_KEY`     | AES-256-GCM key for encrypted secrets (64 hex chars) | `openssl rand -hex 32`                                  |
| `DATABASE_URL`           | PostgreSQL connection string                         | `postgresql://sera_user:sera_pass@sera-db:5432/sera_db` |

### LLM Providers

SERA supports multiple LLM providers simultaneously. Configure them via environment variables or the Settings UI.

=== "Local (LM Studio)"

    ```bash
    LLM_BASE_URL=http://host.docker.internal:1234/v1
    LLM_MODEL=qwen3.5-35b-a3b
    LLM_API_KEY=lm-studio
    ```

=== "Local (Ollama)"

    ```bash
    LLM_BASE_URL=http://host.docker.internal:11434/v1
    LLM_MODEL=llama3.1
    LLM_API_KEY=ollama
    ```

=== "Cloud (OpenAI)"

    ```bash
    OPENAI_API_KEY=sk-...
    # Model auto-detected by name prefix (gpt-*)
    ```

=== "Cloud (Anthropic)"

    ```bash
    ANTHROPIC_API_KEY=sk-ant-...
    # Model auto-detected by name prefix (claude-*)
    ```

=== "Cloud (Google)"

    ```bash
    GOOGLE_API_KEY=AIza...
    # or GEMINI_API_KEY=AIza...
    # Model auto-detected by name prefix (gemini-*)
    ```

!!! tip "Multiple providers"
You can configure multiple providers simultaneously. The `providers.json` file in `core/config/` maps model names to providers. Cloud providers are auto-detected by model name prefix.

### Authentication (OIDC)

SERA supports three authentication modes:

1. **API key only** (default) — set `SERA_BOOTSTRAP_API_KEY`
2. **Bring-your-own IdP** — set `OIDC_ISSUER_URL`, `OIDC_CLIENT_ID`, etc.
3. **Bundled Authentik** — use `bun run prod:auth:up` with the auth compose overlay

```bash
# Bring-your-own IdP
OIDC_ISSUER_URL=https://your-idp.example.com/realms/sera
OIDC_CLIENT_ID=sera-web
OIDC_CLIENT_SECRET=your-client-secret
OIDC_AUDIENCE=sera-api
```

### Embeddings

For semantic memory search, configure an embedding provider:

```bash
# Ollama on host
OLLAMA_URL=http://host.docker.internal:11434
```

The default embedding model is `nomic-embed-text` (768 dimensions). If you change models, Qdrant collections must be recreated.

## YAML Manifest Files

SERA loads configuration from several YAML directories at startup:

| Directory              | Purpose                   | Hot-reload? |
| ---------------------- | ------------------------- | ----------- |
| `templates/builtin/`   | Agent template blueprints | On restart  |
| `sandbox-boundaries/`  | Tier policy definitions   | On restart  |
| `capability-policies/` | Permission grant sets     | On restart  |
| `lists/`               | Named allow/deny lists    | On restart  |
| `circles/`             | Circle definitions        | On restart  |
| `skills/builtin/`      | Skill documents           | Hot-reload  |
| `agents/`              | Agent instance manifests  | On restart  |
| `mcp-servers/`         | MCP server definitions    | On restart  |

These files are imported by `ResourceImporter` on every startup. Changes to YAML files take effect after restarting sera-core (except skills, which hot-reload).

## Docker Compose Profiles

| Command                | Stack                  | Use case                           |
| ---------------------- | ---------------------- | ---------------------------------- |
| `bun run dev:up`       | Dev (hot-reload)       | Local development with live reload |
| `bun run prod:up`      | Production             | Homelab deployment                 |
| `bun run prod:auth:up` | Production + Authentik | Multi-user with SSO                |
