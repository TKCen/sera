# Discord Integration

Connect your SERA agents to Discord for chat and notifications.

## Prerequisites

- A Discord bot application ([Discord Developer Portal](https://discord.com/developers/applications))
- Bot token with `MESSAGE_CONTENT` intent enabled
- The bot invited to your server with appropriate permissions

## Setup

### 1. Store the Bot Token

Bot tokens are stored in SERA's encrypted secrets store — never in environment variables or agent context.

Via the dashboard:

1. Navigate to **Settings** > **Secrets**
2. Click **Add Secret**
3. Name: `discord-bot-token`
4. Paste your bot token
5. Set allowed agents (e.g., `sera`)

Or have Sera request it via the `secrets.requestEntry` tool — she'll trigger an out-of-band entry dialog in the UI.

### 2. Create a Discord Channel

Via the dashboard:

1. Navigate to **Channels**
2. Click **Add Channel**
3. Type: Discord
4. Name: `discord-main`
5. Bot Token Secret: `discord-bot-token`
6. Configure routing (which agents respond to messages)

Via API:

```bash
curl -X POST http://localhost:3001/api/channels \
  -H "Authorization: Bearer $SERA_API_KEY" \
  -d '{
    "type": "discord",
    "name": "discord-main",
    "config": {
      "botTokenSecret": "discord-bot-token"
    }
  }'
```

### 3. Configure Routing

Set up which agents respond to Discord messages:

```bash
curl -X POST http://localhost:3001/api/channels/routing-rules \
  -H "Authorization: Bearer $SERA_API_KEY" \
  -d '{
    "channelId": "{channel-id}",
    "agentId": "{sera-agent-id}",
    "pattern": "*"
  }'
```

## Features

The `DiscordChatAdapter` supports:

| Feature                 | Description                                       |
| ----------------------- | ------------------------------------------------- |
| **DM conversations**    | Users can DM the bot directly                     |
| **Session persistence** | Conversations persist across messages             |
| **Typing indicators**   | Bot shows typing while agent thinks               |
| **Chunked messages**    | Long responses are split into Discord-safe chunks |
| **Slash commands**      | `/ask`, `/status`, `/history`, `/reset`           |

## Notifications

Agents can send notifications to Discord channels:

- Permission request approvals
- Budget alerts
- Task completion notifications
- Circle broadcast messages

Configure alert rules in the dashboard to route specific events to Discord.
