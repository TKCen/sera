# BYOH Python Example

Minimal Python 3 agent that demonstrates the SERA BYOH (Bring Your Own Harness)
contract for **ephemeral** mode. It uses only the Python standard library plus
`requests` — no frameworks required.

## What this example demonstrates

| Contract dimension   | Implementation                                            |
| -------------------- | --------------------------------------------------------- |
| §1 Task receive      | Reads one JSON line from `stdin`                          |
| §2 Result report     | Writes one JSON line to `stdout`                          |
| §3 LLM access        | `requests.post` to `$SERA_LLM_PROXY_URL/chat/completions` |
| §5 Health server     | `http.server.HTTPServer` in a daemon thread               |
| §7 Graceful shutdown | `signal.signal(SIGTERM, ...)`                             |

## Build

```bash
docker build -t sera-byoh-python:latest examples/byoh-python/
```

## Deploy to SERA

```bash
# Import the template
curl -X POST http://localhost:3001/api/templates/import \
  -H "Authorization: Bearer sera_bootstrap_dev_123" \
  -H "Content-Type: application/yaml" \
  --data-binary @examples/byoh-python/sera-template.yaml
```

## Environment variables consumed

| Variable                     | Usage                                                        |
| ---------------------------- | ------------------------------------------------------------ |
| `SERA_IDENTITY_TOKEN`        | Bearer token for LLM proxy and SERA API calls                |
| `SERA_LLM_PROXY_URL`         | Base URL for OpenAI-compatible LLM proxy                     |
| `SERA_CORE_URL`              | Base URL for sera-core (available, not used in this example) |
| `AGENT_CHAT_PORT`            | Port for the health HTTP server (default `3100`)             |
| `AGENT_LIFECYCLE_MODE`       | Must be `ephemeral` for this example                         |
| `HTTP_PROXY` / `HTTPS_PROXY` | Respected automatically by `requests`                        |

## Key points

- The health server starts and `ready` is set to `true` **before** stdin is read,
  so SERA's startup poll sees `ready: true` as soon as the harness is initialised.
- All debug output goes to `stderr`; `stdout` carries only the final result JSON.
- A 429 from the LLM proxy (budget exhausted) is surfaced as a task error rather
  than retried, matching the contract requirement.
- No API keys are embedded — all authenticated calls use `SERA_IDENTITY_TOKEN`.

## See also

- `docs/BYOH-CONTRACT.md` — full contract specification
- `core/agent-runtime/` — TypeScript/Bun reference harness
- `examples/byoh-shell/` — equivalent shell implementation
