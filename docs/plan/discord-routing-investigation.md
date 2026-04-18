# Plan: Discord Message Routing in SERA 2.0

## Problem Statement

The Discord connector receives messages from users but does not route them to the LLM/runtime for processing. The bot appears online but doesn't respond to user messages.

## Current State (Investigation)

Based on code inspection:
- ✅ Discord WebSocket connector exists (`discord.rs`)
- ✅ Connector spawns and connects to Discord Gateway
- ✅ MESSAGE_CREATE events are parsed and sent to `mpsc::Sender<DiscordMessage>`
- ❓ Unknown: Whether Discord messages are forwarded to the runtime/harness
- ❓ Unknown: Whether the runtime processes messages and sends responses back to Discord

## Root Cause Hypotheses

1. **No message forwarding**: The `discord_tx` channel may not be connected to the runtime event processing
2. **Runtime not spawned**: The `sera-runtime` harness may not be running or may not be processing Discord events
3. **Response routing broken**: The runtime may produce responses but not send them back to Discord
4. **Missing wiring**: The gateway may not dispatch Discord messages to the agent turn loop

## Investigation Steps

### Step 1: Verify Discord Connector is Running

```bash
# Check if Discord connector is processing messages
# Look for trace logs showing MESSAGE_CREATE events
```

### Step 2: Verify Runtime Harness is Spawned

```bash
# Check if sera-runtime process is running
ps aux | grep sera-runtime

# Check gateway logs for harness spawn events
```

### Step 3: Trace Message Flow

1. Discord MESSAGE_CREATE → DiscordConnector::handle_payload → mpsc::channel
2. ??? → ??? (where does the channel go?)
3. ??? → Runtime turn loop
4. Runtime response → DiscordConnector::send_message → Discord channel

### Step 4: Identify Missing Wiring

Locate where `discord_tx` should connect to the runtime but doesn't.

## Implementation (If Needed)

Based on investigation findings — don't speculate on implementation until step 3 is complete.

## Verification

- [ ] User mentions @Sera in Discord channel
- [ ] Message appears in gateway/runtime logs
- [ ] LLM processes the message
- [ ] Response sent back to Discord channel
- [ ] User sees the response