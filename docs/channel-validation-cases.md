# Channel Architecture — Validation Use Cases

Ten end-to-end scenarios that exercise the unified ingress/egress channel model across binding modes, session continuity, cross-channel interactions, and built-in/external channel interplay.

---

## 1. Discord DM → agent chat with streamed response

**Binding:** Discord channel in `agent` mode bound to `developer-prime`

**Steps:**
1. User sends "refactor the auth middleware to use decorators" in a Discord DM to the bot
2. `DiscordChannel` produces `IngressEvent { channelId, threadId: dmChannelId, senderId: 'discord:12345', message }`
3. `IngressRouter` resolves binding → `developer-prime`; looks up `channel_sessions` → not found → creates new SERA session + `channel_sessions` row
4. Dispatches to `POST /api/chat` with `{ sessionId, agentId: 'developer-prime', message }`
5. Agent begins reasoning; thought events published
6. `EgressRouter` receives `ChannelEvent { sessionId, eventType: 'chat.response', body: partial tokens }`
7. `DiscordChannel.send()` resolves `sessionId` → `channel_sessions` → `platformThreadId` → edits the "thinking..." message with streamed tokens
8. Agent completes; final response posted as embed in DM

**Validates:**
- Agent binding mode end-to-end
- Session creation on first contact
- Ingress → chat dispatch → egress loop
- Streaming response via session linkage

---

## 2. Discord circle channel → multi-agent party discussion

**Binding:** Discord channel in `circle` mode bound to `engineering` circle

**Steps:**
1. User posts "/sera party should we migrate to Fastify or stay on Express?" in `#engineering`
2. `DiscordChannel` produces `IngressEvent` with the party command
3. `IngressRouter` resolves binding → `engineering` circle; creates session + `channel_sessions` row (threadId = new Discord thread)
4. Circle's party mode triggered: `POST /api/circles/engineering/party` with `{ prompt, sessionId }`
5. Party session runs 2 rounds — `architect`, `developer-prime`, and `qa-agent` each contribute
6. Each agent's response arrives as a `ChannelEvent { sessionId, agentId, eventType: 'chat.response' }`
7. `DiscordChannel.send()` posts each response as a separate embed in the thread, attributed to the agent
8. Synthesis agent posts final summary
9. All messages share the same `sessionId` — full conversation is in one SERA session

**Validates:**
- Circle binding mode
- Party mode orchestration via channel
- Multi-agent responses in a single session
- Thread-based conversation continuity with multiple agents

---

## 3. Schedule fires → agent task → result posted to Discord

**Binding:** `builtin:schedule` bound to `researcher` agent; Discord channel in `notification` mode

**Steps:**
1. Schedule `daily-arxiv-scan` fires (cron: `0 8 * * *`)
2. `builtin:schedule` produces `IngressEvent { channelId: 'builtin:schedule', senderId: 'system', message: 'Run daily arxiv scan', sessionId: new }` with binding `{ type: 'agent', agentId: 'researcher' }`
3. `IngressRouter` creates a fresh session (no conversation continuity for schedules) → dispatches as task
4. `researcher` agent runs, produces a result with paper summaries
5. Task completion emits `ChannelEvent { eventType: 'task.completed', sessionId, agentId: 'researcher', body: summary }`
6. `EgressRouter` evaluates routing rules → matches `task.completed` → routes to Discord notification channel
7. Discord notification channel (notification mode — no threadId resolution needed) posts a rich embed in `#research-updates`

**Validates:**
- Built-in schedule channel as ingress
- Cross-channel flow: ingress via schedule, egress via Discord
- Session created per schedule fire (no continuity)
- Routing rules matching event types to notification channels

---

## 4. Agent-to-agent DM via intercom → circle escalation

**Binding:** `builtin:intercom` in `dynamic` mode

**Steps:**
1. `developer-prime` agent calls the `direct-message` tool targeting `architect`: "I need a schema review for the new migrations"
2. `IntercomService.dm()` publishes to Centrifugo; `builtin:intercom` wraps it as `IngressEvent { channelId: 'builtin:intercom', senderId: 'developer-prime', threadId: 'dm:developer-prime:architect', message }`
3. `IngressRouter` resolves dynamic binding from metadata → target `architect`; looks up `channel_sessions` for this DM pair → creates session
4. `architect` receives the message with session context (can see the full DM history)
5. `architect` decides this needs circle input → calls `circle-broadcast` tool to `engineering` circle
6. `builtin:intercom` produces new `IngressEvent` with `threadId: 'circle:engineering:schema-review'`
7. Circle routing kicks in — message dispatched to all circle members
8. If a Discord channel is bound to `engineering` circle, the broadcast also appears there via `EgressRouter`

**Validates:**
- Built-in intercom channel as ingress
- Agent-to-agent DM session tracking
- Escalation from agent DM to circle broadcast
- Cross-channel egress: intercom event → Discord notification

---

## 5. REST API chat → session continues on Discord

**Binding:** `builtin:api` (dynamic) + Discord channel in `agent` mode bound to `developer-prime`

**Steps:**
1. Operator starts a conversation via the web UI: `POST /api/chat { agentId: 'developer-prime', message: 'set up the new database tables' }` → session `sess-abc` created
2. Agent responds via the web UI (normal flow, `builtin:api` egress = HTTP response)
3. Operator leaves the desk; later sends a message on Discord to the same bot: "what's the status of those tables?"
4. Discord bot produces `IngressEvent { threadId: discordDmId }`
5. `IngressRouter` creates a new `channel_sessions` row → **new session** `sess-def` (Discord thread ≠ web session)
6. Agent responds without the web conversation context (different session)
7. Operator realizes context is lost, uses `/sera continue sess-abc what's the status?`
8. The `/sera continue` command attaches the Discord thread to the existing session: updates `channel_sessions` row to point `sera_session_id → sess-abc`
9. Agent now has full context from the web conversation

**Validates:**
- Cross-channel session handoff (web → Discord)
- Session isolation by default (different channels = different sessions)
- Explicit session linking via slash command
- Context assembly uses session history regardless of originating channel

---

## 6. Webhook ingress → agent task → Slack notification + Discord notification

**Binding:** `builtin:webhook` (dynamic) + Slack channel (notification) + Discord channel (notification)

**Steps:**
1. GitHub sends a `push` event to `POST /api/webhooks/incoming/github-main`
2. `builtin:webhook` validates HMAC signature, produces `IngressEvent { channelId: 'builtin:webhook', message: 'Push to main: 3 commits by alice', metadata: { repo, branch, commits } }`
3. Webhook route table resolves target → `developer-prime` agent
4. `IngressRouter` creates session → dispatches as task: "Review these 3 commits for issues"
5. Agent reviews, produces result
6. Task completion emits `ChannelEvent { eventType: 'task.completed' }`
7. `EgressRouter` matches routing rules:
   - Rule A: `task.completed` + filter `{ source: 'webhook' }` → Slack `#dev-notifications`
   - Rule B: `task.completed` → Discord `#ops-log` (catch-all)
8. Both Slack and Discord receive the result — different formatting per adapter

**Validates:**
- Built-in webhook channel as ingress with HMAC validation
- Webhook → agent task → multi-channel egress fan-out
- Routing rule filters (source-specific vs catch-all)
- Per-adapter formatting (Slack Block Kit vs Discord embed)

---

## 7. Permission request → Discord approval → agent unblocks

**Binding:** Discord channel in `notification` mode (for approvals)

**Steps:**
1. `developer-prime` agent needs access to `/home/user/projects/secret-repo` — calls `POST /api/agents/:id/permission-request { dimension: 'filesystem', value: '/home/user/projects/secret-repo' }`
2. `PermissionRequestService` creates pending request, emits `ChannelEvent { eventType: 'permission.requested', actionable: { approveToken, denyToken }, sessionId: agentSessionId }`
3. `EgressRouter` matches → Discord approval channel
4. `DiscordChannel.send()` posts embed with "Approve" / "Deny" buttons; embed includes agent name, requested path, reason
5. Operator clicks "Approve" → Discord interaction callback → `POST /api/notifications/action { token: approveToken }`
6. Action token validated → `PermissionRequestService.decide('grant', 'session')` → agent unblocks
7. Decision recorded in audit trail: `{ actor: 'operator:alice', dimension: 'filesystem', decision: 'grant', grantType: 'session', channel: 'discord:ops-approvals' }`
8. Confirmation `ChannelEvent { eventType: 'permission.granted' }` posted back to the same Discord embed (edited to show "Approved by alice")

**Validates:**
- Actionable egress events with button callbacks
- Discord interaction → action token → permission service loop
- Audit trail includes the channel that facilitated the decision
- Egress event updates (edit existing message, not new message)

---

## 8. Two Discord bots, same guild, different bindings

**Setup:**
- Bot A: `agent` mode → `developer-prime` (in `#dev-chat`)
- Bot B: `circle` mode → `engineering` circle (in `#engineering`)
- Bot C: `notification` mode (in `#ops-alerts`)

**Steps:**
1. User messages Bot A in `#dev-chat`: "fix the failing tests in auth module"
2. `IngressRouter` routes to `developer-prime` → session created for this thread
3. `developer-prime` starts working, spawns ephemeral subagent `tester`
4. `tester` fails → triggers alert event
5. `EgressRouter` routes alert → Bot C in `#ops-alerts`
6. Meanwhile, user messages Bot B in `#engineering`: "what's the status of the auth refactor?"
7. `IngressRouter` routes to `engineering` circle → circle's primary agent (`architect`) responds with circle-wide context
8. `architect` knows about `developer-prime`'s work (shared circle memory) and reports status
9. All three bots operate independently — different tokens, different channels, different sessions, same SERA instance

**Validates:**
- Multiple channel instances of the same type with different bindings
- Independent session tracking per bot/channel
- Cross-channel information flow via shared circle memory (not via channel plumbing)
- Alert routing to notification-mode channel from agent activity in agent-mode channel

---

## 9. Conversation continuity across agent restart

**Binding:** Discord channel in `agent` mode bound to `developer-prime`

**Steps:**
1. User has an ongoing conversation in a Discord thread (session `sess-xyz`, 15 messages deep)
2. `developer-prime` is restarted (operator runs `/sera restart developer-prime`)
3. Container stops, new container spawns with fresh agent runtime
4. User sends another message in the same Discord thread: "continue where you left off"
5. `DiscordChannel` produces `IngressEvent { threadId: sameThread }`
6. `IngressRouter` looks up `channel_sessions` → finds existing `sess-xyz` → sets `event.sessionId`
7. Chat dispatch loads session history (15 messages) from DB → includes in context assembly
8. Agent responds with full context despite the restart — "I was working on the auth middleware refactor. The last thing I did was..."

**Validates:**
- Session survives agent restart (session is in DB, not in agent memory)
- `channel_sessions` mapping persists across container lifecycle
- Context assembly from session history feeds into the new agent instance
- No special handling needed — the channel model naturally handles this

---

## 10. Email alert → operator replies → task dispatched via API

**Binding:** Email channel in `notification` mode + `builtin:api` (dynamic)

**Steps:**
1. Alert rule fires: `developer-prime` budget at 90% of daily limit
2. `EgressRouter` matches → email channel → sends email to operator
3. Email body contains: summary, current usage, limit, and two links: "View in SERA" (deep link to `/agents/developer-prime`) and "Increase budget" (action token URL)
4. Operator clicks "Increase budget" link → browser opens `POST /api/notifications/action { token }` → budget increased to 2x
5. Operator then opens the SERA web UI → navigates to agent detail → sends a chat message: "resume the analysis with the increased budget"
6. This goes through `builtin:api` → new session → agent gets the message and continues

**Validates:**
- Email as egress-only notification channel
- Action tokens work from email links (not just interactive buttons)
- Cross-channel operator journey: email notification → web UI action → API chat
- Alert rules → egress routing → actionable response loop
