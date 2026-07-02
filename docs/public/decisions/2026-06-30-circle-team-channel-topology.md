# ADR — Circle Team-Channel Topology

> **Status:** ACCEPTED — 2026-06-30
> **Bead:** `sera-nqh3` (Common-goal Circle demo). Anchor for the proof-slice PR #1339.
> **Authors:** SERA product-team lane (resident Circle affordance).
> **Scope:** address/event contract for the resident Circle channel.
> Implementation lives in `rust/crates/sera-types/src/circle_channel.rs` and
> `rust/crates/sera-cli/src/circle_run.rs`. Proof: `sera circle run --to`.

---

## 1. Context

SPEC-circles §3d–§3i describe a *structured* Circle (DAG of members, coordination
policy, ResultAggregator, CircleVerdict). SPEC-circles §5a describes a *loose*
inter-agent `AgentChannel` (subscribers, access policy, broadcast). Neither
gives the operator a way to **address a Circle as a single resident team-channel
target** — analogous to addressing an agent, a session, or a channel — so the
runtime cannot route inbound traffic at a Circle as a first-class destination.

The offline `sera circle run --to` proof slice (PR #1339) introduced a
`CircleChannelAddress` + `CircleChannelEvent` envelope to fix exactly this gap.
This ADR records the address and event contract as the canonical surface, and
aligns code, spec, and the proof seam.

## 2. Decision

A Circle is a **resident team-channel addressed as `to:circle:<name>`**
(e.g. `to:circle:sera-nqh3`). The address is the only ingress handle the
runtime needs to bind an inbound request to a Circle context.

### 2.1 Canonical address

The canonical address form is `to:circle:<name>`, where `<name>` matches
`[A-Za-z0-9._-]{1,128}` (non-empty, no leading/trailing whitespace, no path
separators, no whitespace, max 128 chars).

The parser accepts three equivalent inputs and **normalizes to the canonical
form on display and serialization**:

| Input              | Accepted | Normalized display |
|--------------------|----------|---------------------|
| `to:circle:<name>` | yes      | `to:circle:<name>`  |
| `circle:<name>`    | yes      | `to:circle:<name>`  |
| `<name>` (bare)    | yes      | `to:circle:<name>`  |
| `to:<other>:<x>`   | **rejected** (`UnsupportedKind`) | — |
| `to:circle` (no name) | **rejected** (`InvalidName`) | — |
| `to:circle:<bad>` (path sep / whitespace / >128 chars) | **rejected** (`InvalidName`) | — |

Rationale: the bare-name and `circle:` shorthands are convenient for operator
input and for fixture/CLI ergonomics; canonicalizing on parse keeps the
`Display` and serde string-codec output stable. Rejecting any other `kind:`
prefix is deliberate — the `to:` envelope is forward-compatible for future
channel kinds (e.g. `to:agent:<id>`), and we do not silently coerce them.

### 2.2 Serializer stability

`CircleChannelAddress` is encoded through `serde(try_from = "String", into = "String")`
— the JSON form is the literal canonical address string
(e.g. `"to:circle:sera-nqh3"`). This makes addresses portable across
serializers, copy/paste safe, and round-trip stable without an envelope struct.
New struct fields must be added with `#[serde(default = "...")]` to preserve
backwards compatibility.

### 2.3 Event vocabulary

`CircleChannelEvent` is a tagged enum (`#[serde(tag = "kind", rename_all = "snake_case")]`)
with the following kinds. The list is intentionally tiny for the first slice;
new kinds can be added without breaking round-trip serialization of existing
kinds, but **must** remain stable once they ship (operators index by `kind`).

| `kind`         | Purpose                                                                                              | Required fields                                              |
|----------------|------------------------------------------------------------------------------------------------------|--------------------------------------------------------------|
| `claim_role`   | A member claims a role inside the Circle (lead, worker, critic, referee).                            | `member: String`, `role: "lead"\|"worker"\|"critic"\|"referee"` |
| `post`         | A member posts a payload contribution (proposal, objection, verdict body, …).                       | `member: String`, `artifact_type: String`, `summary: String` |
| `receipt`      | An objective-binding receipt proving the contribution was emitted by the channel and is reproducible. | `member: String`, `action: String`, `tokens: Option<u64>`    |
| `lineage`      | A directed lineage edge in the proposal-DAG (criticises, resolves, derives_from).                     | `member: String`, `from_entry_id: u64`, `to_entry_id: u64`, `relation: String` |
| `verdict`      | A referee verdict, optionally accompanied by rationale.                                             | `member: String`, `verdict: String`, `rationale: String`     |

The minimum contract asks for `claim_role`, `post`, `receipt`, `verdict`.
`lineage` is **additional proof vocabulary** that already exists in the
offline slice — it carries the proposal-DAG relationship from the
`LineageEdge` proof validator (`rust/crates/sera-types/src/circle_validator.rs`)
into the channel event log so round-trip from event log to proof bundle is
loss-free.

### 2.4 Roles

`CircleChannelRole` is `{"lead" | "worker" | "critic" | "referee"}` (snake_case).
Default labels (used in `ProofBundleMember.role`) are `Lead`, `Worker`,
`Critic`, `Referee/Integrator`. Referee/Integrator is a single role with a
composite label because the integrator closeout (SPEC-circles §3c LeadReviewer
aggregator) is a referee concern in the offline slice.

## 3. Relationship to other specs

- **SPEC-circles §3d–§3i** — the structured Circle model. The channel address
  is *how you reach* a Circle; the coordination model is *what it does* once
  reached. This ADR does not change §3d–§3i; it adds the missing ingress
  handle.
- **SPEC-circles §5a (Inter-Agent Communication Channels)** — the loose
  `AgentChannel` (subscribers, broadcast) is a different concept (Phase 4+).
  This ADR does **not** modify §5a; the `to:circle:<name>` address is a
  *team-channel* (one Circle, one address), not a broadcast topic.
- **`docs/public/specs/SPEC-circles.md` §6** — the YAML manifest format does
  not yet pin a `circle_id` / channel address field. The minimal follow-up is
  to add an optional `address: to:circle:<name>` field to the Circle manifest
  schema; tracked as a separate doc delta and out of scope for this ADR.
- **`2026-06-28-common-goal-agent-collaboration.md`** — the upstream
  common-goal framing this slice is the offline proof of. No change.

## 4. Proof seam

The contract is exercised end-to-end by the offline `sera circle run --to`
proof slice (`sera circle run --to <name|to:circle:NAME>`):

1. Parses `--to` through `CircleChannelAddress::parse`.
2. Builds a deterministic `Vec<CircleChannelEvent>` covering `claim_role`,
   `receipt`, and `verdict` kinds (per-member + per-referee).
3. Assembles a `CollaborationProofBundle` and round-trips it through
   `sera circle validate`, which checks mixed-provider, verdict, lineage, and
   blank-response rules.

The slice is the **audit/replay surface** for the channel-event contract; it is
not the runtime ingress. The resident runtime ingress lands in the
`resident-circle-routing` follow-up (PM plan Task C) and reuses this contract
unchanged.

## 5. Acceptance criteria satisfied by this ADR

- Canonical `to:circle:<name>` parse/display exists in tracked code
  (`rust/crates/sera-types/src/circle_channel.rs:81-182`) and is test-covered
  (`tests` block at `rust/crates/sera-types/src/circle_channel.rs:311-435`).
- `circle:<name>` and bare `<name>` are accepted and normalize to canonical
  (§2.1, tests `parse_circle_kind_without_to_prefix`,
  `parse_bare_name_auto_prefixes_to_circle`,
  `circle_run_cli_accepts_short_circle_address_and_bare_name`).
- Event vocabulary covers `claim_role`, `post`, `receipt`, `verdict`; `lineage`
  is documented as additional proof vocabulary (§2.3).
- Contract is serializer-stable (`String`-codec serde, round-trip tests).
- `sera circle run --to <name>` offline proof still passes (20/20 tests in
  `rust/crates/sera-cli/tests/circle_run.rs`).
- SPEC-circles is left unchanged; a follow-up doc delta for §6 manifest
  schema is tracked but not required for this ADR.

## 6. Non-goals

- No resident runtime ingress — that is PM plan Task C.
- No `to:agent:<id>` / `to:team:<id>` / `to:session:<id>` envelope siblings
  here. The `to:` envelope is forward-compatible, but new kinds land in
  separate ADRs and code modules.
- No change to SPEC-circles §5a (loose `AgentChannel`). It is a different
  primitive and remains Phase 4+.

## 7. Task C: resident Circle ingress seam (2026-07-02)

This section documents the resident-runtime ingress path that turns the
address contract above into an in-process run/session state. It is
narrow on purpose: the smallest seam that proves
`to:circle:<name>` is reachable as a resident runtime target and that
its run/session state carries Circle identity and lineage. Broader
behavior — autonomous decomposition, external connectors, dynamic
scheduling, referee closeout exporting back into proof bundles —
lands in PM plan Tasks D/E and is out of scope for this section.

### 7.1 In-process ingress path

Two new modules implement the seam, both thin on top of the contract
types above:

- `rust/crates/sera-types/src/circle_ingress.rs` defines
  `CircleIngressRequest`, `CircleIngressCaller`, and `CircleIngressRecord`
  — the typed request wrapper callers hand in and the bounded run/session
  record the funnel returns. Lineage rides on the existing
  `sera_types::runtime::TurnContext.parent_session_key` field; no parallel
  lineage field is invented.
- `rust/crates/sera-runtime/src/circle_ingress.rs` defines
  `CircleIngress` (the funnel) and `CircleIngressOutcome`. `accept` parses
  the address exactly once via `CircleChannelAddress::parse`, builds a
  `TurnContext` whose `session_key` is `session:circle:<name>:<member-id>`,
  emits at most `CIRCLE_INGRESS_DEFAULT_EVENT_LIMIT` (`32`) bounded channel
  events (a `ClaimRole` and a `Post`), and records one entry in the supplied
  `SharedCircleActivityLog` when present.

### 7.2 Scope boundary (what is explicitly NOT here)

- No LLM calls.
- No DB I/O — the only state mutators are the in-process channel-event list
  (returned in the record) and an optional `SharedCircleActivityLog`.
- No external connector or platform delivery.
- No autonomous decomposition or member dispatch — `CircleIngress::accept`
  produces one bounded record per call.
- No HTTP route / gateway wiring — that lands in PM plan Task E. The CLI
  `sera circle run --to` proof seam remains the single user-facing
  surface; the runtime ingress is a library seam usable from future tasks.

### 7.3 Lineage invariants

- A root ingress call (no `parent_session_key` on the request) produces a
  `TurnContext.parent_session_key = None`.
- A child ingress call (with `parent_session_key`) propagates the value
  verbatim onto the returned `TurnContext`. The `session_key` is derived
  fresh from `(address, member_id)` — lineage is *not* encoded into the
  session key string.

### 7.4 Acceptance criteria satisfied by this Task C slice

- Tracked, typed in-process ingress path exists
  (`rust/crates/sera-runtime/src/circle_ingress.rs::CircleIngress::accept`).
- It accepts the parser-supported `to:circle:<name>` forms (canonical,
  `circle:<name>`, bare `<name>`) via the shared `CircleChannelAddress::parse`.
- It produces a bounded run/session state (`CircleIngressRecord`) carrying
  Circle identity, address, channel events, and a `TurnContext` with
  lineage preserved via `parent_session_key`.
- Member-role context and recent peer activity are represented through the
  existing `SharedCircleActivityLog` and `CircleChannelEventKind` (no new
  primitive vocabulary introduced).
- Happy-path test: `accept_canonical_address_returns_record_with_two_events`
  in `circle_ingress.rs`.
- Existing Task B contract tests still pass (`cargo test -p sera-types --lib circle_channel`).
- Existing offline `sera circle run --to` proof still passes
  (`cargo test -p sera-cli --test circle_run`).
- This ADR was extended (§7) without rewriting §1–§6.

### 7.5 Follow-up explicitly NOT done here

- Wiring the funnel into a gateway route (Task E).
- Recording the channel events back into a `CollaborationProofBundle`
  (Task D — proof closeout).
- Adding a `parent_task_id`-style mirror to the ingress seam (the existing
  `parent_session_key` field is sufficient for the seam documented in
  §7.1; if a later ADR or task adds `parent_task_id`, it should mirror the
  existing convention rather than invent a third lineage channel).

---

## 8. Task D — resident Circle proof closeout and referee export seam

> **Bead:** `sera-nqh3` follow-up. Builds directly on §7 (Task C) without
> rewriting §1–§7.

### 8.1 Context

§7 (Task C) introduced a small in-process ingress funnel that turns a
`to:circle:<name>` request into a bounded `CircleIngressRecord` — channel
events, lineage, and a `TurnContext`. What it deliberately did **not** do
is close that record out into a referee-signed proof artifact the operator
can inspect or replay. The offline `sera circle run --to` proof slice
already produces a `CollaborationProofBundle` and `sera circle validate`
already checks it offline; resident evidence therefore had no
first-class export path that round-trips through the same validator.

### 8.2 Decision

Add a small, deterministic, library-only **proof closeout** helper in
`rust/crates/sera-runtime/src/circle_ingress.rs` that:

1. Takes a `CircleIngressRecord` (Task C output) plus a small
   `CircleIngressCloseout` struct (`referee_member_id`, `verdict`,
   `rationale`, optional `objective`).
2. Synthesises a `CollaborationProofBundle` whose entries map one-to-one
   onto the record's `channel_events` plus a final referee verdict entry.
3. Emits one `ExecutionReceipt` per entry, with a provider-class rotation
   (alternating local / cloud) that mirrors the offline `sera circle run`
   schema so a small two-or-three-entry bundle still satisfies the
   `validate_proof_bundle` mixed-provider check.
4. Wires a `Resolves` lineage edge from the last pre-referee entry into
   the verdict entry — Task D's explicit "referee closeout" beat.
5. Validates the bundle with `sera_types::circle_validator::validate_proof_bundle`
   inside the helper and refuses to return a bundle that fails
   validation (errors are surfaced as a `String` with the validator's
   failure kinds).
6. Reuses the existing `request_id` (first 12 hex chars) plus
   `address.name` to derive the bundle's `run_id`, so the closeout is
   *accountable to the resident record* and not a re-invented id.

The helper does **not**:

- Mutate the resident record.
- Open a network or DB connection.
- Schedule, decompose, or recursively expand the run.
- Touch the gateway/operator route (out of scope; that is Task E).
- Duplicate the address parser; it still reuses
  `CircleChannelAddress::parse` and the existing `CircleChannelEvent` /
  `CircleChannelEventKind` types.

### 8.3 Member identity, lineage, and receipts

Bundle author disambiguation:

- The validator's `check_receipts_for_provider_entries` builds a
  `HashMap<executor, Option<provider>>` keyed on the entry's
  `author`. A single resident caller (`alice`) appears in two events
  (ClaimRole + Post) with the same `member_id`, so the HashMap would
  silently keep the *last* `alice` receipt and report
  `ReceiptProviderEvidenceMismatch` for the first entry.
- Task D disambiguates at the bundle boundary by appending `@evt<idx>`
  to the bundle `author` (e.g. `alice@evt0`, `alice@evt1`) while
  keeping the original `member_id` in the entry's payload under
  `author` for human readability. The referee verdict entry uses
  `<referee>@verdict`. This matches the offline seam's pattern of
  unique `participant_id`s without inventing a new convention.

Determinism:

- The anchor timestamp `t0` is
  `2026-07-02T09:00:00Z + circle_id.len() seconds + (request_id % 1e9) ns`.
  Identical `(circle, request_id)` pairs therefore close out into
  byte-identical bundle bytes — required for the existing
  `sera circle run` style deterministic proof tests.
- The provider rotation is `idx % 2 == 0 → local, else → cloud` and
  aligns with `sera-cli/src/circle_run.rs::provider_for`. With two
  channel events plus a verdict entry that gives us entries at idx
  0 (local), 1 (cloud), 2 (local again) — still satisfying the
  validator's `has_local && has_cloud` check.

### 8.4 Failure states are inspectable, not silently swallowed

`SharedCircleActivityLog::record` now returns
`ActivityLogRecordOutcome::{Recorded, Poisoned}`. `CircleIngress::accept`
uses that outcome to either increment `activity_writes` honestly or
return `Err(CircleIngressError::ActivityLogPoisoned)`, discarding the
in-progress channel events / `TurnContext` so the caller never sees a
partial record where the events were written but the activity log
wasn't. This is the wired-in form of the Task C review's
*non-blocking* follow-up #1: the error variant is no longer dead.

The invalid-address no-mutation regression was also strengthened: it
now asserts on `total_entries()` (across all circles) in addition to
the old `recent_for_circle("bob", ...)` filter, so a regression that
writes to a *different* circle id can no longer pass.

### 8.5 Acceptance criteria satisfied by this Task D slice

- Resident run produces inspectable proof entries (one per channel
  event plus the referee verdict) tied to the caller's member id
  (carried in the entry payload as `author`).
- A referee / integrator closeout path is explicit in the proof
  model: `CircleIngressCloseout` is a stable input type with
  `referee_member_id` / `verdict` / `rationale` (and optional
  `objective`), the bundle's `verdict` reviewer is the supplied
  referee, and a `Resolves` lineage edge ties the last pre-referee
  entry into the verdict entry.
- Receipts and lineage from `CircleIngressRecord` survive export —
  the same `request_id` shows up in the bundle's `run_id`, and the
  `channel_events` are reflected one-to-one in the bundle's `entries`.
- `validate_proof_bundle` (the function the `sera circle validate`
  CLI command wraps) passes on the exported bundle. The
  `closeout_bundle_passes_validate_proof_bundle` test runs that
  check directly; running `cargo test -p sera-cli --test circle_run`
  covers the same `validate_proof_bundle` function on a CLI-built
  bundle, so the equivalent validation seam is exercised for both
  the resident closeout and the offline CLI flow.
- Failure states are inspectable: `ActivityLogRecordOutcome::Poisoned`
  surfaces as `CircleIngressError::ActivityLogPoisoned` and the
  strengthened `invalid_address_is_rejected_without_mutation` test
  catches both wrong-circle-id and same-circle-id regressions.
- Existing Task B/C regressions stay green
  (`cargo test -p sera-types --lib circle_channel`,
  `cargo test -p sera-runtime --lib circle`,
  `cargo test -p sera-cli --test circle_run`).

### 8.6 Follow-up explicitly NOT done here

- Wiring the closeout helper into a gateway route (Task E).
- A `sera circle closeout` CLI subcommand (would be redundant — the
  helper is the equivalent library seam for this slice).
- Persistence (DB, file) of the closeout bundle — the helper is
  intentionally in-process and pure; downstream callers can
  `serde_json::to_vec_pretty(&out.bundle)` to write it.
- Mixed-shape ingress records (records with non-`ClaimRole`-`Post`
  event pairs). The closeout helper now enforces a **canonical-only
  guard** at the top of `closeout_into_proof_bundle`
  (`is_canonical_claim_role_post_shape`) that returns a hard error
  when the supplied record is not exactly two events, the first a
  `ClaimRole` and the second a `Post`. The wider
  `CircleChannelEventKind` matcher in the entry-building loop is kept
  only as defence-in-depth; it never runs on the operator seam today
  (see §9 for the canonical-only boundary that the operator surface
  preserves).

## 9. Task E — resident Circle operator observability surface

> **Bead:** `sera-nqh3` follow-up. Builds directly on §7 (Task C)
> and §8 (Task D) without rewriting §1–§8.

### 9.1 Context

§7 + §8 delivered the resident ingress funnel and the proof closeout /
referee export helper, but neither of those is addressable from an
operator surface. The offline `sera circle run --to` command is the
operator's existing seam for Circle work, but it does not exercise the
resident funnel — there was no path that proved a Circle was being
**addressed as a resident topology** rather than as an offline replay.
Task E adds that one narrow seam and ties the audit hooks around it.

### 9.2 Decision

Add one narrow operator-facing function in
`rust/crates/sera-runtime/src/circle_ingress.rs` —
`address_circle(funnel, request, closeout, audit_limit) -> Result<
OperatorCloseoutReport, String>` — that:

1. Calls `CircleIngress::accept` on the supplied request, surfacing
   `CircleIngressError::InvalidAddress` and `ActivityLogPoisoned`
   verbatim as `String` errors so the operator sees the underlying
   failure kind, not a wrapper.
2. Calls `closeout_into_proof_bundle` (the §8 helper) on the
   resulting `CircleIngressRecord`. The §8 canonical-only guard
   fires loudly when the record's `channel_events` are not exactly
   `ClaimRole` + `Post` from one caller — Task E does **not**
   generalise this slice.
3. Re-runs `validate_proof_bundle` on the emitted bundle and exposes
   the validator's verdict as `report.validation: Result<(), Vec<String>>`
   so the operator surface never depends on the helper's hidden
   "already validated" contract.
4. Re-uses the funnel's `SharedCircleActivityLog` (when attached) to
   gather `recent_for_circle(circle_id, "nobody", Some(audit_limit))`
   for the addressed circle. This deliberately avoids
   `total_entries()` so a poisoned mutex cannot silently zero the
   operator's view of recent activity (carryforward #2 from the
   Task D review).
5. Returns an `OperatorCloseoutReport` (serialisable to JSON) with
   every required field exposed under a stable snake_case key:
   `circle_id`, `address`, `member_id`, `role`, `session_key`,
   `parent_session_key`, `request_id`, `run_id`, `verdict_reviewer`,
   `verdict_type`, `verdict_rationale`, `entry_count`,
   `receipt_count`, `lineage_edge_count`, `bundle_sha256`,
   `run_id_t0`, `validation`, `audit_tail`, `activity_writes`. The
   full `CollaborationProofBundle` is also surfaced (under
   `report.bundle`, `#[serde(skip)]`) so a CLI can write the
   canonical bundle file to disk and re-validate it via
   `sera circle validate --bundle` without re-running the helper.

A `sera circle closeout` CLI subcommand
(`rust/crates/sera-cli/src/circle_closeout.rs`) is wired on top of
this single function. The command accepts `--to`, `--member`,
`--role`, `--summary`, `--parent-session-key`, `--agent-id`,
`--referee`, `--verdict`, `--rationale`, `--objective`,
`--bundle-out`, `--report-out`, `--audit-limit`, and `--json`. It
emits a text summary or a structured JSON summary, an optional
bundle file, an optional report file, and a `circle-closeout:` machine
footer on every exit (including usage errors) so log parsers share
one footer vocabulary with `sera circle run`.

### 9.3 Canonical-only boundary (carryforward from Task D review)

The Task D review noted that the §8 helper was *over-broad*: its
matcher accepted `Receipt`, `Lineage`, and `Verdict` event variants
even though the only ingress shape §7 produces is the
`ClaimRole + Post` pair. Task E:

- Refuses non-canonical records inside the §8 helper itself
  (the `is_canonical_claim_role_post_shape` guard), so the operator
  seam can never silently emit a bundle whose roster/role
  attribution no longer matches the input.
- Documents this guard in §8.6 above, and explicitly does **not**
  broaden the operator surface to multi-author ingress records in
  this slice. Future work that wants broader ingress shapes must
  either preserve roster/role semantics or carve out a new
  non-canonical-only proof schema.

### 9.4 Activity-log accounting (carryforward #2)

The Task D review also flagged that `SharedCircleActivityLog::
total_entries()` silently returns `0` on a poisoned mutex. Task E:

- Does **not** rely on `total_entries()` for runtime/operator
  diagnostics. `address_circle` reads recent activity via
  `recent_for_circle`, which (a) is scoped to the addressed circle
  only and (b) gracefully degrades to an empty `Vec` if the mutex
  is poisoned, instead of lying about a count.
- Surfaces the funnel's own `activity_writes` counter on the report
  so the operator can spot a poisoned write directly. `0` means
  "no log attached" **or** "log attached but the write was
  poisoned"; the operator surface does not collapse these two cases
  into one another — the underlying `accept` already returns
  `ActivityLogPoisoned` for the second case before `address_circle`
  can return a report.

### 9.5 Provider rotation — reconciled prose (carryforward #3)

§8 §8.3 still describes the §8 helper's provider rotation as
"aligned with `sera-cli/src/circle_run.rs::provider_for`". Task E
keeps that claim honest by stating the actual relationship in this
ADR (the helper code's docstring was already updated in §8's
slice):

- The helper's `closeout_provider_for(idx)` returns
  `local` for even `idx` and `cloud` for odd `idx`.
- The offline CLI's `provider_for` starts on `cloud` and alternates
  on `idx`.
- The exact ordering therefore differs between the two seams, but
  the validator's `MixedLocalCloud` check only requires *both*
  classes to appear — both seams satisfy it for any bundle with
  ≥ 2 entries. Do not assume byte-for-byte equivalence.

### 9.6 Out-of-scope (explicit guardrails)

The operator surface is the small truthful slice. It does **not**:

- Spawn sub-tasks or call `delegate_task` / `kanban_create` /
  Beads.
- Talk to Discord, Slack, or any messaging platform.
- Open a network connection, hit a gateway route, or require a
  live runtime restart. The CLI command is purely offline.
- Persist anything to a database or filesystem beyond the
  caller-supplied `--bundle-out` / `--report-out` paths.
- Recursively expand the run into a multi-agent swarm or a
  scheduler job.
- Generalise beyond the canonical §7 ingress shape. Multi-author
  ingress records are out of scope (see §9.3).
- Expose `total_entries()` to the operator (see §9.4).

### 9.7 Acceptance criteria satisfied by this Task E slice

- One narrow operator-facing path exists (`sera circle closeout`
  in `rust/crates/sera-cli/src/circle_closeout.rs`,
  backed by `address_circle` in
  `rust/crates/sera-runtime/src/circle_ingress.rs`).
- The report exposes canonical circle id/address, member id/role,
  session lineage (`session_key` + `parent_session_key` +
  `request_id`), proof `run_id`, verdict reviewer / type /
  rationale, and validation status. Every required field has a
  stable snake_case JSON key.
- Guardrails (§9.6) are written into the docstring of the CLI
  command and the §9 ADR section.
- The path uses `CircleIngress::accept` and
  `closeout_into_proof_bundle` rather than reassembling those
  concepts. It does not duplicate `CircleChannelAddress::parse`
  or invent a parallel validator.
- Existing Task B/C/D regressions stay green (see §9.8).
- This ADR delta documents the shipped behavior.

### 9.8 Verification commands

All of these pass on the clean-base worktree at the merge of this
slice:

```bash
cargo test -p sera-runtime --lib circle_ingress     # 20 tests
cargo test -p sera-runtime --lib                    # 712 tests
cargo test -p sera-types --lib circle_ingress       # 7 tests
cargo test -p sera-types --lib circle_channel       # 13 tests
cargo test -p sera-cli --test circle_run            # 20 tests
cargo test -p sera-cli --test circle_closeout       # 5 tests
cargo check -p sera-runtime --lib
```
