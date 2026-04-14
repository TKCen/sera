# SERA Discord Integration Testing Guide

## Pre-requisites

### Discord Bot Setup
1. **Create Discord Application**: Visit https://discord.com/developers/applications
2. **Create Bot User**: Navigate to "Bot" section and create a bot
3. **Copy Bot Token**: Save the bot token securely
4. **Enable Intents**: Enable "Message Content Intent" in Bot settings
5. **Generate Invite URL**: 
   - Go to "OAuth2" > "URL Generator"
   - Select scopes: `bot`
   - Select permissions: `Send Messages`, `Read Message History`
6. **Invite to Server**: Use generated URL to invite bot to your Discord server

### LLM Provider Setup (LM Studio Example)
1. **Install LM Studio**: Download from https://lmstudio.ai/
2. **Load Model**: Download and load a model (e.g., Gemma 2B)
3. **Start Server**: Enable "Local Server" on port 1234
4. **Verify**: Check http://localhost:1234/v1/models returns model list

## Deployment Steps

### 1. Configure Discord Token
```bash
# Replace YOUR_ACTUAL_BOT_TOKEN with the token from Discord Developer Portal
./rust/target/debug/sera secrets set connectors/discord-main/token "YOUR_ACTUAL_BOT_TOKEN"
```

### 2. Verify Configuration
```bash
# Check that token is stored
./rust/target/debug/sera secrets list

# Should show: connectors/discord-main/token
```

### 3. Start SERA Gateway
```bash
# Use the deployment script
./deploy-discord.sh

# OR run manually with detailed logging
RUST_LOG=debug ./rust/target/debug/sera start --config sera.yaml --port 3001
```

## Testing the Discord Connection

### 1. Verify Gateway Startup
Look for these log messages:
```
✅ Discord connector started successfully:
INFO sera_gateway::discord: Starting Discord connector connector=discord-main agent=sera
INFO sera_gateway: Discord connector exited with error: ... (if there's an issue)
```

### 2. Test Discord Bot Response
1. **Find Bot in Server**: Your bot should appear in the member list
2. **Send Test Message**: In a channel where the bot has access, type: `Hello @YourBotName`
3. **Expected Flow**:
   - Bot receives message via Discord Gateway WebSocket
   - Message is queued for processing
   - SERA runtime processes the message using configured LLM
   - Response is sent back to Discord channel

### 3. Check HTTP API
```bash
# Check health endpoint
curl http://localhost:3001/health

# List sessions (requires auth if SERA_API_KEY is set)
curl http://localhost:3001/api/sessions
```

## Troubleshooting

### Discord Connection Issues

**Bot Not Responding:**
- ✅ Check bot has "Send Messages" permission in channel
- ✅ Check "Message Content Intent" is enabled
- ✅ Verify bot token is correct: `sera secrets get connectors/discord-main/token`
- ✅ Check Discord Developer Portal > Bot > Token for correctness

**WebSocket Connection Errors:**
- Look for logs: `Discord connector exited with error`
- Common issues: Invalid token, missing intents, network connectivity

**Permission Denied:**
- Check channel permissions for bot role
- Ensure bot is not restricted by server roles

### LLM Provider Issues

**Local LM Studio:**
- ✅ Verify LM Studio server is running: `curl http://localhost:1234/v1/models`
- ✅ Check model is loaded and inference works
- ✅ Verify port 1234 is not blocked by firewall

**Remote Provider:**
- Update `sera.yaml` provider configuration
- Set appropriate API key if required

### Runtime Issues

**Agent Not Spawning:**
- Check logs for `Failed to spawn runtime harness`
- Verify `sera-runtime` binary is built: `cargo build --bin sera-runtime`
- Check that runtime can access LLM provider

**Database Issues:**
- SQLite database is created automatically at `sera.db`
- Check file permissions if running on restricted filesystem

## Example Session Flow

1. **User in Discord**: `@sera hello, what can you do?`
2. **SERA Gateway**: Receives Discord message via WebSocket
3. **Session Manager**: Creates/finds session for user+channel
4. **Runtime**: Processes message with LLM and configured tools
5. **Response**: Sent back to Discord channel
6. **Logging**: All interactions logged with session tracking

## Monitoring

### Key Log Messages
- `Starting Discord connector`: Connection established
- `Spawned runtime harness`: Agent ready to process messages
- `Event processing loop started`: Ready for Discord events
- `Starting HTTP server`: API available

### Debug Mode
```bash
RUST_LOG=debug ./rust/target/debug/sera start
```

This provides verbose logging of:
- Discord WebSocket messages
- Runtime communication
- Session state transitions
- Tool execution details