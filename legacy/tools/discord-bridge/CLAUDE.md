# sera-discord-bridge

Minimal TypeScript sidecar that bridges Discord to the SERA Rust core API.

## What it does

- Listens for Discord DMs and @mentions via discord.js
- Forwards messages to `POST /api/chat` on the SERA core (default `http://localhost:3001`)
- Replies with the agent's response, splitting messages that exceed Discord's 2000-char limit
- Session IDs are scoped per-channel: `discord-{channelId}`

## Setup

```bash
cp .env.example .env
# Edit .env — at minimum set DISCORD_TOKEN
npm install
npm start
```

## Env vars

| Variable        | Default                                    | Required |
| --------------- | ------------------------------------------ | -------- |
| `DISCORD_TOKEN` | —                                          | Yes      |
| `SERA_CORE_URL` | `http://localhost:3001`                    | No       |
| `SERA_API_KEY`  | `sera_bootstrap_dev_123`                   | No       |
| `SERA_AGENT_ID` | `e39b5569-f110-49a2-99c5-25758872a958`     | No       |

## Discord bot permissions

Create a bot at https://discord.com/developers/applications and enable:

- **Intents:** Server Members, Message Content (both must be enabled in the portal)
- **Bot permissions:** Send Messages, Read Message History

## Architecture note

This is a standalone sidecar — it does NOT import any code from `core/`.
The existing `DiscordChatAdapter.ts` in `core/src/channels/` is an in-process
adapter with session storage and slash commands; this bridge is intentionally
simpler (HTTP forwarding only).
