# SERA Configuration System

## Manifest Format

SERA uses **K8s-style YAML manifests** with four MVS kinds: Instance, Provider, Agent, Connector.

### Document Structure

```yaml
apiVersion: sera.dev/v1
kind: Instance|Provider|Agent|Connector
metadata:
  name: <string>
spec:
  # Kind-specific fields...
```

### Single-File Mode

Multiple manifests in one file are separated by `---`:

```yaml
---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: my-sera
spec:
  tier: local
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: lm-studio
spec:
  kind: openai-compatible
  base_url: "http://localhost:1234/v1"
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: lm-studio
  # ...
```

The config loader splits on `---` and parses each document separately.

## Four MVS Kinds

### 1. Instance
Root configuration object (one per sera.yaml).

```yaml
kind: Instance
metadata:
  name: my-sera
spec:
  tier: local                         # local, docker, kubernetes
  context_window: 8192                # LLM context limit
  max_iterations: 10                  # Max turns per request
```

### 2. Provider
LLM provider connection details.

```yaml
kind: Provider
metadata:
  name: lm-studio
spec:
  kind: openai-compatible             # or: anthropic, openai, gemini
  base_url: "http://localhost:1234/v1"
  default_model: "gemma-4-12b"
  api_key:
    secret: "providers/lm-studio/api-key"
```

**Secret format**: `{ secret: "path" }` — resolved to `SERA_SECRET_<PATH>` env var (/ and - become _).

### 3. Agent
A reasoning agent bound to a provider and toolset.

```yaml
kind: Agent
metadata:
  name: sera
spec:
  provider: lm-studio                 # Name of Provider kind
  model: "gemma-4-12b"                # Override provider default
  persona:
    immutable_anchor: |
      You are Sera, a helpful assistant.
      Complete tasks using the available tools.
  tools:
    allow: ["memory_*", "file_*", "shell", "session_*"]  # Glob patterns
```

**Persona**: System prompt with immutable anchor (required) + dynamic hooks.

**Tools**: Glob-pattern allow list. Patterns like `memory_*` match `memory_read`, `memory_write`, `memory_search`.

### 4. Connector
Integration with external messaging platform (Discord in MVS).

```yaml
kind: Connector
metadata:
  name: discord-main
spec:
  kind: discord                       # Only kind supported in MVS
  token:
    secret: "connectors/discord-main/token"
  agent: sera                         # Agent to route messages to
  intents: 37377                      # Discord intents bitmask
```

**Secret token** resolved from `SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN`.

## Secret Resolution

### Environment Variable Mapping

Path `connectors/discord-main/token` → `SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN`

Rules:
- Prefix: `SERA_SECRET_`
- Replace `/` with `_`
- Replace `-` with `_`
- Convert to UPPERCASE

Example:
```yaml
secret: "providers/openai/api-key"
# Resolves to: SERA_SECRET_PROVIDERS_OPENAI_API_KEY
```

### In Code

```rust
use sera_config::manifest_loader::resolve_provider_api_key;

let provider_spec: ProviderSpec = manifests.provider_spec("lm-studio")?;
let api_key = resolve_provider_api_key(&provider_spec)?;
```

## ManifestSet API

The loader returns a `ManifestSet` struct with typed access:

```rust
let manifests = load_manifest_file("sera.yaml")?;

// Typed access
let agent_spec: AgentSpec = manifests.agent_spec("sera").ok().flatten()?;
let provider_spec: ProviderSpec = manifests.provider_spec("lm-studio").ok().flatten()?;

// List all
let agent_names: Vec<&str> = manifests.agent_names();
let providers: Vec<ProviderManifest> = &manifests.providers;
```

## Config Layering (POST-MVS)

MVS uses file + environment vars only. Full layering (post-MVS):

1. **Defaults** — Hardcoded in spec types (context_window: 8192, max_iterations: 10)
2. **File** — sera.yaml (takes precedence over defaults)
3. **Environment** — `SERA_*` vars (takes precedence over file)
4. **Runtime** — HTTP API patches (takes precedence over all)

MVS skips layer 4 (no runtime patches API).

## Multi-Document YAML Parsing

`serde_yaml` doesn't handle multi-document YAML natively. The loader:

1. Reads entire file as string
2. Splits on `^---$` (line-boundary `---`)
3. Parses each document separately
4. Collects into appropriate vectors by kind

```rust
fn parse_manifests(yaml_text: &str) -> Result<ManifestSet> {
    let docs: Vec<&str> = yaml_text
        .split("---")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    // Parse each doc...
}
```

## Example: Complete sera.yaml

```yaml
---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: my-sera
spec:
  tier: local
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: openai
spec:
  kind: openai-compatible
  base_url: "https://api.openai.com/v1"
  api_key:
    secret: "providers/openai/api-key"
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: local-llm
spec:
  kind: openai-compatible
  base_url: "http://localhost:1234/v1"
  default_model: "meta-llama/llama-2-7b"
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: local-llm
  model: "meta-llama/llama-2-7b"
  persona:
    immutable_anchor: |
      You are Sera, a helpful AI assistant.
      Complete the user's request using available tools.
  tools:
    allow: ["memory_*", "file_*", "shell", "session_*"]
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: researcher
spec:
  provider: openai
  model: "gpt-4-turbo"
  persona:
    immutable_anchor: |
      You are a research assistant. Provide detailed, well-cited responses.
  tools:
    allow: ["file_*", "web_fetch", "memory_*"]
---
apiVersion: sera.dev/v1
kind: Connector
metadata:
  name: discord-main
spec:
  kind: discord
  token:
    secret: "connectors/discord-main/token"
  agent: sera
```

## Environment Setup

To run with this config:

```bash
export SERA_SECRET_PROVIDERS_OPENAI_API_KEY="sk-..."
export SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN="NzA...MTQ1"

sera start -c sera.yaml -p 3001
```

---

Last updated: 2026-04-09
