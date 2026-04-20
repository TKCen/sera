# Local agent — minimal end-to-end walk-through

Runs one SERA agent backed by any OpenAI-compatible LLM endpoint (LM Studio, Ollama, vLLM, OpenAI, Anthropic). Uses the default local profile — **no Postgres, no Centrifugo, no Docker required**.

## 1. Prerequisites

- Rust **1.94+** (edition 2024)
- An OpenAI-compatible LLM endpoint on `localhost` or elsewhere

## 2. Build

```bash
cd rust
cargo build --release --bin sera
```

The binary lands at `rust/target/release/sera`.

## 3. Start the gateway

```bash
export SERA_LLM_BASE_URL=http://localhost:1234/v1       # LM Studio default
export SERA_LLM_API_KEY=not-needed-for-local-endpoints
./rust/target/release/sera start
```

State writes to `./sera.db` (SQLite) and `./logs/`. No migrations to run — `sera-gateway` handles schema bootstrap on first boot.

## 4. Talk to the agent

In another shell:

```bash
./rust/target/release/sera-cli auth login     # device-code flow; dev token works for local
./rust/target/release/sera-cli agent list
./rust/target/release/sera-cli chat --agent <agent-id>
```

The chat REPL streams tokens via SSE and shows tool calls inline. Ctrl-D exits.

## 5. Peek at the TUI (optional)

```bash
./rust/target/release/sera-tui
```

Agent list, active sessions, HITL approvals, evolve-proposal status.

## What just happened

- `sera` booted the gateway and spawned an agent-runtime worker.
- The worker's four-method turn loop (`_observe → _think → _act → _react`) invoked the LLM, emitted `Submission` / `Event` frames over the in-process transport, and wrote a hash-chained audit trail to `sera-telemetry`.
- Your chat input went through the gateway, into the runtime's Submission queue, and the streamed reply came back as `Event::StreamingDelta` frames.

## Next steps

- **Add a tool** — drop a manifest in `tools/` and reference it from the agent's persona
- **Add a hook** — register one of the 20 `HookPoint` variants to observe or gate turns
- **Switch to enterprise profile** — set `DATABASE_URL` and the gateway auto-promotes to Postgres + pgvector
- **Add a skill** — skill authors read `docs/plan/specs/SPEC-plugins.md`

## Troubleshooting

- **`sera-cli auth login` hangs** — the dev token `sera_bootstrap_dev_123` works for local loops; pass it via `SERA_API_KEY`
- **Port 8080 busy** — set `SERA_GATEWAY_PORT=8180` before `sera start`
- **LM Studio rejects empty api_key** — set `SERA_LLM_API_KEY` to any non-empty string
