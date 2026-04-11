# SERA Discord Integration

## Protocol Overview

SERA connects to Discord via **raw WebSocket to the gateway** (`wss://gateway.discord.gg/?v=10`). The connector handles the full gateway protocol and dispatches messages to the reasoning loop.

## Gateway Connection Flow

### 1. Initial Connection
```
Client connects to wss://gateway.discord.gg/?v=10
```

### 2. Receive HELLO
Discord sends:
```json
{
  "op": 10,
  "d": {
    "heartbeat_interval": 45000
  }
}
```

**Opcode 10** = HELLO. Tells client heartbeat interval in milliseconds.

### 3. Send IDENTIFY
Client sends:
```json
{
  "op": 2,
  "d": {
    "token": "Bot NzA...",
    "intents": 37377,
    "properties": {
      "os": "linux",
      "browser": "sera",
      "device": "sera"
    }
  }
}
```

**Opcode 2** = IDENTIFY.

- `token` — Discord bot token (with "Bot" prefix)
- `intents` — Bitmask of event subscriptions
- `properties` — Client metadata

### 4. Heartbeat Loop
Client sends heartbeat every `heartbeat_interval` ms:
```json
{
  "op": 1,
  "d": null
}
```

**Opcode 1** = HEARTBEAT. Keep-alive ping.

### 5. Receive DISPATCH
Discord sends messages as events:
```json
{
  "op": 0,
  "t": "MESSAGE_CREATE",
  "s": 1234,
  "d": {
    "id": "message_id",
    "channel_id": "channel_id",
    "guild_id": "guild_id",
    "author": { "id": "user_id", "username": "alice" },
    "content": "hello sera"
  }
}
```

**Opcode 0** = DISPATCH.

- `t` — Event type (MESSAGE_CREATE, etc.)
- `d` — Event payload

## MVS Intents

SERA subscribed to intents bitmask **37377** which combines:

```
GUILDS (1)
GUILD_MESSAGES (1 << 9)
DIRECT_MESSAGES (1 << 12)
MESSAGE_CONTENT (1 << 15)
```

Binary: `1001000011000001` = 37377

**Why MESSAGE_CONTENT?** Without it, message.content is empty if bot is not mentioned.

## Message Dispatch

### Filter
1. Receive MESSAGE_CREATE event
2. Check if message is from a bot → skip
3. Check channel type (guild or DM) → handle both

### Queue
Message forwarded to async mpsc channel:
```rust
struct DiscordMessage {
    user_id: String,
    username: String,
    channel_id: String,
    guild_id: Option<String>,
    content: String,
}

// Sender in DiscordConnector holds this:
tx.send(DiscordMessage { ... }).await?;
```

### Event Loop
Separate task consumes queue:
```rust
async fn event_loop(state: Arc<AppState>, mut rx: mpsc::Receiver<DiscordMessage>) {
    while let Some(msg) = rx.recv().await {
        // Find agent for connector
        // Get/create session with session_key = format!("discord:{}", msg.channel_id)
        // Execute turn
        // Send reply via REST API
    }
}
```

### Reply
Send reply back to Discord via REST API:
```rust
POST https://discordapp.com/api/channels/{channel_id}/messages
{
  "content": "Your reply text"
}
```

## Reconnection

On WebSocket close:
1. Log error
2. Sleep 5 seconds
3. Reconnect to gateway
4. Resume or restart (MVP uses restart)

```rust
loop {
    match connect_to_gateway(&token).await {
        Ok(ws) => {
            if let Err(e) = gateway_loop(ws, &tx).await {
                eprintln!("Gateway loop failed: {}", e);
            }
        }
        Err(e) => {
            eprintln!("Connection failed: {}", e);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
```

## Code Structure

### DiscordConnector

```rust
pub struct DiscordConnector {
    token: String,
    agent_name: String,
    tx: mpsc::Sender<DiscordMessage>,
}

impl DiscordConnector {
    pub fn new(token: &str, agent_name: &str, tx: mpsc::Sender<DiscordMessage>) -> Self { ... }
    
    /// Main entry point: connect to gateway and run loop.
    pub async fn run(&self) -> Result<(), Box<dyn Error>> { ... }
    
    /// Send a message to a Discord channel (REST API).
    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<()> { ... }
}
```

### ser.rs Integration

In `sera-core/src/bin/sera.rs`:

```rust
// 1. Load Discord connector from manifests
for connector_manifest in &state.manifests.connectors {
    let spec: ConnectorSpec = serde_json::from_value(cm.spec.clone())?;
    if spec.kind != "discord" { continue; }
    
    let token = resolve_connector_token(&spec)?;
    let agent_name = spec.agent.unwrap_or("sera").to_owned();
    
    // 2. Spawn connector task
    let connector = DiscordConnector::new(&token, &agent_name, discord_tx.clone());
    tokio::spawn(async move {
        if let Err(e) = connector.run().await {
            tracing::error!("Discord connector exited: {}", e);
        }
    });
}

// 3. Spawn event processing loop
tokio::spawn(async move {
    event_loop(event_state, discord_rx).await;
});
```

## Error Handling

### Token Resolution
If `SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN` is not set:
```
Warn: Discord token not resolved (set SERA_SECRET_* env var). Skipping connector.
```

### Connection Errors
Logged and retried with 5-second delay. Does not crash server.

### Message Processing Errors
If turn execution fails, error logged but connector stays alive.

### Rate Limiting
Discord enforces rate limits on message sends. Recommended: queue messages with backoff (post-MVS).

## Configuration

In sera.yaml:

```yaml
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

Environment:
```bash
export SERA_SECRET_CONNECTORS_DISCORD_MAIN_TOKEN="NzA..."
sera start
```

## Post-MVS Enhancements

- **Resume** — Reconnect with same session_id instead of restart
- **Intent negotiation** — Query per-guild permissions
- **Slash commands** — Handle INTERACTION_CREATE for `/slash` commands
- **Buttons/modals** — Rich interactions beyond text
- **Rate limit handling** — Automatic backoff and queue
- **Multi-guild** — Handle cross-guild message routing
- **Thread support** — Messages in threads (nested channels)

---

Last updated: 2026-04-09
