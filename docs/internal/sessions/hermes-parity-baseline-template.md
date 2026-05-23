# Hermes Parity Baseline — Operator Live Smoke Report

> Companion to:
>
> - `docs/internal/plans/SERA-HERMES-PARITY-DIFFERENTIATION-PLAN-2026-05-23.md` (plan)
> - `docs/internal/plans/hermes-parity-matrix.md` (parity matrix)
> - `rust/crates/sera-e2e-harness/tests/hermes_parity_baseline.rs` (CI/regression gate)
>
> **Public-safe.** Do not paste credentials, real Discord IDs, home-network details, or operator chat content into this file.

The CI gate (`hermes_parity_baseline_gate`) covers the three core probes on a
freshly booted gateway with a scripted mock LLM. This report is the place
to capture an **operator-run live capture** against a real upstream
(MiniMax / local Qwen) plus the optional Discord one-turn smoke — those
probes need credentials and a live process, so they sit outside the
automated test by design.

Copy this template into a timestamped sibling file (e.g.
`hermes-parity-baseline-2026-05-23.md`) before running, redacting any
private values.

---

## Capture metadata

| Field | Value |
|---|---|
| Capture date (UTC) | `YYYY-MM-DD HH:MM` |
| Operator | redacted |
| Branch / commit SHA | _e.g. `omc/hermes-parity-matrix-baseline @ <sha>`_ |
| Gateway version / build | `cargo run -p sera-gateway -- --version` |
| Runtime version / build | `cargo run -p sera-runtime -- --version` |
| Provider chain | `minimax` primary, `local-qwen` fallback (redact endpoints) |

---

## Probe 1 — Readiness

```bash
curl -fsS "http://localhost:3001/api/health/ready" | jq .
```

| Expectation | Result |
|---|---|
| HTTP 200 | _PASS / FAIL_ |
| `runtime_connected=true` | _PASS / FAIL_ |
| `harness_ready=true` (if exposed) | _PASS / FAIL_ |

Notes: _redact private IPs / hostnames; capture failure class verbatim if FAIL._

---

## Probe 2 — Authenticated `/api/chat` nonce

```bash
NONCE="nonce-$(date +%s%N)"
curl -fsS -X POST "http://localhost:3001/api/chat" \
  -H "Authorization: Bearer $SERA_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"agent\": \"sera\", \"message\": \"Reply with OK and the nonce $NONCE.\", \"stream\": false}" \
  | jq '{session_id, response, usage}'
```

| Expectation | Result |
|---|---|
| HTTP 200 | _PASS / FAIL_ |
| `session_id` present + non-empty | _PASS / FAIL_ |
| `response` present + non-empty | _PASS / FAIL_ |
| `usage` present (provider accounting) | _PASS / FAIL_ |

Notes: _capture the `response` body in a redacted form (truncate to first 200 chars). Do not paste raw provider error bodies — extract failure class only._

---

## Probe 3 — Response sanitizer contract

For the same chat call above, the operator-visible `response` MUST NOT
contain raw chain-of-thought tags. The gateway enforces this with
[`response_sanitizer::sanitize_assistant_response`](../../../rust/crates/sera-gateway/src/response_sanitizer.rs),
which is exercised by two layers:

- Unit tests in `rust/crates/sera-gateway/src/response_sanitizer.rs`
  cover balanced blocks, orphan halves, case variants, multi-line bodies,
  and whitespace handling.
- Integration test
  `rust/crates/sera-e2e-harness/tests/hermes_parity_response_sanitizer.rs`
  boots the full gateway + runtime + scripted mock LLM that deliberately
  emits a `<think>…</think>` block, and asserts the operator-visible
  `response` is stripped before it crosses the boundary.

Operator smoke check (against a real provider):

```bash
echo "$RESPONSE_BODY" | grep -iE '<think>|</think>' && echo "LEAK" || echo "CLEAN"
```

| Expectation | Result |
|---|---|
| No `<think>` marker in `response` | _PASS / FAIL_ |
| No `</think>` marker in `response` | _PASS / FAIL_ |
| No raw `reasoning_content` field bleed | _PASS / FAIL_ |
| Sanitization event logged when a reasoning model is in use | _PASS / FAIL_ |

Notes: when sanitization is applied the gateway emits a `tracing::info!`
line with `stripped_blocks > 0` plus the raw / sanitized byte lengths;
the `response_sent` audit row also carries `sanitized_blocks` and
`raw_response_len` so operators can confirm reasoning-model output is
being normalised. Streaming (`/api/chat` with `"stream": true`) is the
open follow-up — deltas can split a `<think>` tag across SSE chunks, so
a stream-aware sanitizer is tracked as `sera-sanitizer-stream` in the
matrix's recommended new beads. If leakage occurs in the non-streaming
path, file under `sera-yj18` and link the redacted capture.

---

## Optional Probe 4 — Discord one-turn smoke

Only run when Discord credentials are available and the operator wants a
live Discord regression. Do **not** paste the bot token, real channel IDs,
or operator/peer Discord handles. Refer to the Discord-related beads
(`sera-yeg.2`, `sera-s77a`) for the capability under test.

| Expectation | Result |
|---|---|
| Mention is acked with `👀` reaction or typing indicator | _PASS / FAIL_ |
| Final reply lands in the configured channel | _PASS / FAIL_ |
| Reply contains no `<think>` markers | _PASS / FAIL_ |
| On forced failure, `❌` reaction + actionable failure class | _PASS / FAIL_ |

Notes: _redact every channel ID and handle; describe the channel by role
(e.g. "private operator channel") only._

---

## Optional Probe 5 — Tool-failure visibility

Trigger a controlled tool failure (e.g. read a nonexistent path through
the file tool) and assert the failure surfaces to the operator with a
typed failure class. Tracks `sera-tqzd`.

| Expectation | Result |
|---|---|
| Operator-facing reply names a typed failure class | _PASS / FAIL_ |
| Audit ledger row written for the failed tool call | _PASS / FAIL_ |
| No silent "looks fine" reply when a tool failed | _PASS / FAIL_ |

---

## Overall verdict

- **Baseline GREEN** when Probes 1–3 all pass on a fresh gateway boot.
- **Baseline RED** when any of Probes 1–3 fail; file/update the owning
  bead with the redacted evidence above before retrying.

Source beads referenced from this capture: `sera-m1k8`, `sera-duo3`,
`sera-q66q`, `sera-ctag`, `sera-s77a`, `sera-tqzd`, `sera-rj4z`,
`sera-yeg.2`, `sera-yj18`.
