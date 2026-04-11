# Epic 26: Extended Channel Adapters

## Overview

Epic 18 establishes the unified channel model and delivers Discord, Slack, Email, and Webhook adapters. This epic extends SERA's channel reach to the messaging platforms where people actually live — Telegram, WhatsApp, Signal, Matrix, and iMessage (via BlueBubbles). Each adapter implements the `Channel` interface (Epic 18 Story 18.1) and integrates with the existing ingress/egress routing, session continuity, and DM pairing systems.

The goal is channel breadth — making SERA agents reachable on any platform the operator uses, with full bidirectional chat, media support, and actionable notifications.

## Context

- **Reference implementation:** OpenClaw `src/channels/plugins/` — 25+ channel adapters loaded as plugins with a catalog, binding registry, and per-channel config schema
- **OpenClaw's approach:** Each channel is a plugin with a manifest (`plugin.json`) declaring capabilities, config schema, and supported message types. Channels are discovered, installed, and configured via CLI or UI.
- **SERA's advantage:** SERA's `Channel` interface with `BindingMode` (agent/circle/notification/dynamic) provides richer routing than OpenClaw's flat bindings. Session continuity via `channel_sessions` gives SERA proper conversation threading across all platforms.
- **Dependency:** All adapters in this epic require Epic 18 Story 18.1 (Channel interface) and Story 18.7 (inbound routing) to be complete

## Dependencies

- Epic 18 (Integration Channels) — Channel interface, IngressRouter, EgressRouter, channel_sessions
- Epic 16 (Auth & Secrets) — SecretsProvider for API tokens/keys
- Epic 25 (Media Processing) — processing media attachments from channels

---

## Stories

### Story 26.1: Telegram channel adapter

**As** an operator
**I want** SERA to work as a Telegram bot with full chat, media, and notification support
**So that** I can interact with agents via Telegram — the most popular messaging app for bot interactions

**Prerequisites (manual, outside SERA):**

- Create a Telegram bot via [@BotFather](https://t.me/BotFather) — get the bot token
- Store the bot token as a SERA secret

**Acceptance Criteria:**

- [ ] `TelegramChannel` adapter using `grammy` (or `telegraf`) library
- [ ] `config`: `{ botTokenSecret: string, allowedChatIds?: string[], webhookUrl?: string }`
- [ ] **Polling mode** (default): long-polling for development and NAT-behind setups
- [ ] **Webhook mode** (optional): `POST /api/channels/telegram/:channelId/webhook` for production — verified via Telegram's webhook secret
- [ ] **Chat (agent/circle binding):**
  - Private messages (DMs) and group messages routed based on binding mode
  - Reply-based threading: replies to bot messages continue the same SERA session
  - Long messages chunked (Telegram 4096-char limit)
  - Typing indicator shown while agent is processing
  - Markdown formatting preserved (Telegram MarkdownV2)
- [ ] **Media support:**
  - Receive: photos, voice messages, audio files, video, documents (PDF)
  - Photos/voice/audio → forwarded to Media Processing Pipeline (Epic 25)
  - Send: text, photos (inline), documents as file attachments
- [ ] **Commands:**
  - `/start` — welcome message with agent/circle info
  - `/help` — list available commands
  - `/status` — agent status (if bound to an agent)
  - `/stop` — clear session context
  - Custom commands registered via BotFather command list
- [ ] **Notifications (notification binding):**
  - Informational: plain text with severity emoji prefix
  - Actionable: inline keyboard buttons for Approve/Deny
  - Callback queries handled → execute action token
- [ ] **Polls integration:**
  - Agents can create Telegram native polls via `create-poll` tool
  - Poll results collected and returned to agent as tool result
- [ ] **DM pairing (if enabled):**
  - Unknown senders in private chat receive pairing challenge
  - Pairing code displayed, operator approves via SERA UI
  - Approved users added to allowlist per channel
- [ ] **Resilience:**
  - Automatic reconnection on network errors
  - Graceful degradation if Telegram API is rate-limited (exponential backoff)
  - Message delivery confirmation tracked

---

### Story 26.2: WhatsApp channel adapter (via WhatsApp Business API)

**As** an operator
**I want** SERA agents accessible via WhatsApp
**So that** I can interact with agents on the world's most-used messaging platform

**Prerequisites (manual, outside SERA):**

- WhatsApp Business API access (Meta Business Suite or BSP like Twilio/MessageBird)
- Configure webhook URL pointing to sera-core
- Store API credentials as SERA secrets

**Acceptance Criteria:**

- [ ] `WhatsAppChannel` adapter using WhatsApp Cloud API (Meta's official API)
- [ ] `config`: `{ phoneNumberId: string, accessTokenSecret: string, verifyToken: string, webhookPath?: string }`
- [ ] **Webhook receiver:** `POST /api/channels/whatsapp/:channelId/webhook` with Meta signature verification
- [ ] **Chat:**
  - Text messages routed via binding mode
  - Session tracked per conversation (sender phone number)
  - 24-hour messaging window: responses within 24h of last user message (WhatsApp policy)
  - Template messages for re-engagement outside the 24h window
- [ ] **Media support:**
  - Receive: images, audio, video, documents, location, contacts
  - Media downloaded from WhatsApp CDN → Media Processing Pipeline
  - Send: text, images, documents
- [ ] **Interactive messages:**
  - Actionable notifications use WhatsApp interactive buttons (max 3)
  - List messages for multi-option selections
- [ ] **Read receipts:** mark messages as read after agent processing
- [ ] **Rate limiting:** respect WhatsApp throughput limits (80 messages/second for standard tier)

---

### Story 26.3: Signal channel adapter (via signal-cli or signald)

**As** an operator
**I want** SERA agents accessible via Signal
**So that** I can use the most privacy-focused messaging platform for sensitive agent interactions

**Prerequisites (manual, outside SERA):**

- Signal account registered with a phone number
- `signal-cli` or `signald` running as a daemon (Docker sidecar)
- Store Signal credentials as SERA secrets

**Acceptance Criteria:**

- [ ] `SignalChannel` adapter using `signal-cli` REST API (via `bbernhard/signal-cli-rest-api` Docker image)
- [ ] `config`: `{ signalApiUrl: string, phoneNumber: string }`
- [ ] **Chat:**
  - Direct messages and group messages routed via binding mode
  - Session per conversation (sender number / group ID)
  - Typing indicators
  - Message reactions for lightweight feedback
- [ ] **Media support:**
  - Receive: images, audio, files
  - Send: text, images, files as attachments
- [ ] **Notifications:**
  - Text-based notifications with severity formatting
  - Actionable: reply-based approval ("reply APPROVE or DENY")
- [ ] **E2E encryption:** All Signal messages are E2E encrypted by design — SERA never sees plaintext except at the adapter boundary
- [ ] **Docker sidecar:** `signal-cli-rest-api` added to docker-compose as optional service

---

### Story 26.4: Matrix channel adapter

**As** an operator
**I want** SERA agents accessible via Matrix (Element, etc.)
**So that** I can use a decentralized, self-hostable messaging platform for agent interactions

**Prerequisites (manual, outside SERA):**

- Matrix homeserver (self-hosted Synapse/Dendrite or matrix.org)
- Create a bot user on the homeserver
- Store access token as SERA secret

**Acceptance Criteria:**

- [ ] `MatrixChannel` adapter using `matrix-bot-sdk`
- [ ] `config`: `{ homeserverUrl: string, accessTokenSecret: string, userId: string }`
- [ ] **Chat:**
  - Room-based: each bound room maps to an agent/circle
  - Thread support: Matrix threads map to SERA sessions
  - Markdown/HTML formatted responses
  - Typing indicators
- [ ] **Media support:**
  - Receive: images, files via Matrix content repository
  - Send: text, images, files uploaded to Matrix content repository
- [ ] **Notifications:**
  - Room notifications with severity-based formatting
  - Reaction-based approval (thumbs up/down on notification messages)
- [ ] **Encryption (optional):**
  - Olm/Megolm E2E encryption support via `matrix-bot-sdk` crypto
  - Disabled by default (requires key management setup)
- [ ] **Federation-friendly:** Works across federated Matrix homeservers

---

### Story 26.5: iMessage channel adapter (via BlueBubbles)

**As** an operator with a Mac
**I want** SERA agents accessible via iMessage
**So that** I can interact with agents through Apple's messaging ecosystem

**Prerequisites (manual, outside SERA):**

- Mac running BlueBubbles server (always-on Mac Mini or similar)
- BlueBubbles configured with API access
- Store BlueBubbles API password as SERA secret

**Acceptance Criteria:**

- [ ] `iMessageChannel` adapter using BlueBubbles REST API
- [ ] `config`: `{ blueBubblesUrl: string, passwordSecret: string }`
- [ ] **Chat:**
  - DM routing via binding mode
  - Session per conversation (contact handle)
  - Read receipts
  - Typing indicators
- [ ] **Media support:**
  - Receive: images, audio, files
  - Send: text, images
- [ ] **Limitations documented:**
  - Requires always-on Mac with BlueBubbles
  - Group iMessage: read-only (BlueBubbles limitation for sending to groups)
  - No interactive buttons (iMessage doesn't support bot interactions)
- [ ] **Notifications:**
  - Text-only notifications
  - Reply-based approval ("reply YES or NO")

---

### Story 26.6: Channel adapter plugin architecture

**As** a community developer
**I want** to build and distribute custom SERA channel adapters as plugins
**So that** SERA can connect to any messaging platform without core changes

**Acceptance Criteria:**

- [ ] `ChannelPlugin` interface extending the base `Channel` interface (Epic 18):

  ```typescript
  interface ChannelPlugin extends Channel {
    manifest: ChannelPluginManifest;
  }

  interface ChannelPluginManifest {
    id: string; // e.g. 'sera-channel-telegram'
    name: string; // human label
    version: string;
    channelType: string; // unique type string
    description: string;
    configSchema: JSONSchema; // JSON Schema for adapter config
    capabilities: {
      chat: boolean;
      media: ('image' | 'audio' | 'video' | 'document')[];
      interactiveButtons: boolean;
      polls: boolean;
      reactions: boolean;
      threads: boolean;
      typing: boolean;
      readReceipts: boolean;
    };
    requiredSecrets: string[]; // list of secret names the adapter needs
    setupGuide: string; // Markdown instructions for prerequisites
  }
  ```

- [ ] Plugin discovery: `ChannelManager` scans `plugins/channels/` directory for installed adapters
- [ ] Each adapter is an npm package with `sera-channel-plugin` in `package.json` keywords
- [ ] `sera channel install <package>` CLI command (Epic 15) installs a channel plugin
- [ ] `sera channel list` shows installed channel adapters with capabilities
- [ ] Plugin lifecycle: `install` → `configure` (via UI wizard, Story 18.9) → `start` → `healthCheck`
- [ ] Plugin isolation: adapters run in sera-core process but errors in one adapter don't crash others (try/catch boundaries)

---

## DB Schema

No new tables — all channel adapters use the existing `notification_channels`, `channel_sessions`, and `notification_routing_rules` tables from Epic 18. Each adapter's config is stored as encrypted JSONB in `notification_channels.config`.

## Docker Compose additions

```yaml
# Optional sidecar for Signal adapter (Story 26.3)
sera-signal-bridge:
  image: bbernhard/signal-cli-rest-api:latest
  environment:
    - MODE=native
  volumes:
    - signal_data:/home/.local/share/signal-cli
  networks:
    - sera_net
  profiles:
    - signal # only started when Signal channel is configured
```
