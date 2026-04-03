# BYOH Shell Example

Minimal POSIX shell agent that demonstrates the SERA BYOH (Bring Your Own Harness)
contract for **ephemeral** mode. It requires only `curl`, `jq`, and `socat` — all
available in a stock Alpine image.

## What this example demonstrates

| Contract dimension   | Implementation                                        |
| -------------------- | ----------------------------------------------------- |
| §1 Task receive      | Reads one JSON line from `stdin` via `cat`            |
| §2 Result report     | Writes one JSON line to `stdout` via `jq -n`          |
| §3 LLM access        | `curl` POST to `$SERA_LLM_PROXY_URL/chat/completions` |
| §5 Health server     | Background `socat` loop serving `/health` responses   |
| §7 Graceful shutdown | `trap cleanup TERM INT`                               |

## Build

```bash
docker build -t sera-byoh-shell:latest examples/byoh-shell/
```

## Deploy to SERA

```bash
# Import the template
curl -X POST http://localhost:3001/api/templates/import \
  -H "Authorization: Bearer sera_bootstrap_dev_123" \
  -H "Content-Type: application/yaml" \
  --data-binary @examples/byoh-shell/sera-template.yaml
```

## Environment variables consumed

| Variable                     | Usage                                            |
| ---------------------------- | ------------------------------------------------ |
| `SERA_IDENTITY_TOKEN`        | Bearer token passed in `Authorization` header    |
| `SERA_LLM_PROXY_URL`         | Base URL for OpenAI-compatible LLM proxy         |
| `AGENT_CHAT_PORT`            | Port for the health HTTP server (default `3100`) |
| `AGENT_LIFECYCLE_MODE`       | Must be `ephemeral` for this example             |
| `HTTP_PROXY` / `HTTPS_PROXY` | Respected automatically by `curl`                |

## Key points

- `socat` runs in a background loop to handle repeated health polls from SERA.
  State (ready/busy) is communicated via a temp file shared between the main
  shell and the socat subprocess.
- `jq` is used for both JSON parsing (task input) and JSON construction (result
  output), ensuring correct escaping of arbitrary string content.
- `curl` respects `HTTP_PROXY`/`HTTPS_PROXY` automatically — no extra flags needed.
- A 429 from the LLM proxy is surfaced as a task error rather than retried.
- All debug output goes to `stderr`; `stdout` carries only the final result JSON.
- The `ENTRYPOINT` uses exec form (`["/app/agent.sh"]`) so Docker sends SIGTERM
  directly to the shell process (PID 1) rather than a wrapper.

## See also

- `docs/BYOH-CONTRACT.md` — full contract specification
- `core/agent-runtime/` — TypeScript/Bun reference harness
- `examples/byoh-python/` — equivalent Python implementation
