/**
 * DiscordChatAdapter — bidirectional Discord chat bound to a specific SERA agent.
 *
 * Unlike the legacy DiscordAdapter (global, primary-agent only), this adapter:
 * - Routes messages to a specific agent instance (configurable via channel config)
 * - Persists conversation history via SessionStore
 * - Enforces guild and user allowlists for security
 * - Supports DMs and @mentions
 * - Chunks long responses to stay under Discord's 2000-char limit
 *
 * Connects via raw WebSocket to the Discord Gateway (no discord.js dependency).
 */

import crypto from 'node:crypto';
import WebSocket from 'ws';
import { Logger } from '../../lib/logger.js';
import type { Orchestrator } from '../../agents/Orchestrator.js';
import type { SessionStore } from '../../sessions/SessionStore.js';
import type { ChatMessage } from '../../agents/types.js';

const logger = new Logger('DiscordChatAdapter');

const DISCORD_API = 'https://discord.com/api/v10';
const DISCORD_GATEWAY = 'wss://gateway.discord.gg/?v=10&encoding=json';
const MAX_MESSAGE_LENGTH = 2000;

// Gateway intents:
// GUILDS (1<<0)=1, GUILD_MESSAGES (1<<9)=512, DIRECT_MESSAGES (1<<12)=4096, MESSAGE_CONTENT (1<<15)=32768
const INTENTS = 1 + 512 + 4096 + 32768; // 37377

export interface DiscordChatConfig {
  botToken: string;
  applicationId: string;
  targetAgentId: string;
  allowedGuilds?: string[];
  allowedUsers?: string[];
  allowDMs?: boolean;
  allowMentions?: boolean;
  responsePrefix?: string;
}

interface DiscordMessagePayload {
  id: string;
  channel_id: string;
  guild_id?: string;
  content: string;
  author: {
    id: string;
    username: string;
    bot?: boolean;
  };
  mentions?: Array<{ id: string }>;
}

interface DiscordInteraction {
  id: string;
  token: string;
  type: number; // 2 = APPLICATION_COMMAND
  channel_id: string;
  guild_id?: string;
  member?: { user: { id: string; username: string } };
  user?: { id: string; username: string }; // DM interactions
  data?: {
    name: string;
    options?: Array<{ name: string; value: string | number }>;
  };
}

/** Discord Application Command definitions for bulk overwrite */
const SLASH_COMMANDS = [
  {
    name: 'ask',
    description: 'Send a message to Sera',
    options: [
      {
        name: 'message',
        description: 'The message to send',
        type: 3, // STRING
        required: true,
      },
    ],
  },
  {
    name: 'status',
    description: 'Show agent status and current session info',
  },
  {
    name: 'history',
    description: 'Show recent conversation history',
    options: [
      {
        name: 'count',
        description: 'Number of messages to show (default 10, max 50)',
        type: 4, // INTEGER
        required: false,
        min_value: 1,
        max_value: 50,
      },
    ],
  },
  {
    name: 'reset',
    description: 'Start a new conversation session',
  },
  {
    name: 'agents',
    description: 'List available agent instances with status',
  },
  {
    name: 'switch',
    description: 'Switch the bound agent for your session',
    options: [
      {
        name: 'agent',
        description: 'Agent instance name or ID',
        type: 3, // STRING
        required: true,
      },
    ],
  },
  {
    name: 'help',
    description: 'Show available commands',
  },
];

export class DiscordChatAdapter {
  private ws: WebSocket | null = null;
  private heartbeatInterval: NodeJS.Timeout | null = null;
  private lastSequence: number | null = null;
  private running = false;
  private botUserId: string | null = null;

  /** Maps Discord userId → SERA sessionId for conversation continuity */
  private userSessions = new Map<string, string>();

  /** Per-user agent override (userId → agentId). Falls back to config.targetAgentId. */
  private userTargetAgent = new Map<string, string>();

  /** Resolve the effective target agent for a user, respecting per-user overrides. */
  private getTargetAgent(userId: string): string {
    return this.userTargetAgent.get(userId) ?? this.config.targetAgentId;
  }

  constructor(
    private channelId: string,
    private config: DiscordChatConfig,
    private orchestrator: Orchestrator,
    private sessionStore: SessionStore
  ) {}

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  async start(): Promise<void> {
    this.running = true;

    // Warn if both message paths are disabled — bot will appear online but be deaf
    if (this.config.allowDMs === false && this.config.allowMentions === false) {
      logger.warn(
        `Discord chat adapter for agent ${this.config.targetAgentId}: ` +
          `both allowDMs and allowMentions are false — no messages will reach the agent! ` +
          `Set at least one to true.`
      );
    }

    this.connect();
    logger.info(
      `Discord chat adapter started for agent ${this.config.targetAgentId} ` +
        `(guilds: ${this.config.allowedGuilds?.length ?? 'all'}, ` +
        `users: ${this.config.allowedUsers?.length ?? 'all'}, ` +
        `DMs: ${this.config.allowDMs !== false ? 'yes' : 'no'}, ` +
        `mentions: ${this.config.allowMentions !== false ? 'yes' : 'no'})`
    );
  }

  async stop(): Promise<void> {
    this.running = false;
    if (this.heartbeatInterval) {
      clearInterval(this.heartbeatInterval);
      this.heartbeatInterval = null;
    }
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this.userSessions.clear();
    logger.info(`Discord chat adapter stopped for channel ${this.channelId}`);
  }

  // ── Gateway Connection ─────────────────────────────────────────────────────

  private connect(): void {
    logger.info('Connecting to Discord Gateway...');
    this.ws = new WebSocket(DISCORD_GATEWAY);

    this.ws.on('open', () => {
      logger.info('Discord Gateway connection opened');
    });

    this.ws.on('message', (data: WebSocket.Data) => {
      try {
        const payload = JSON.parse(data.toString()) as {
          op: number;
          d: unknown;
          s: number | null;
          t: string | null;
        };
        this.handlePayload(payload);
      } catch (err) {
        logger.error('Failed to parse Discord payload:', (err as Error).message);
      }
    });

    this.ws.on('close', () => {
      logger.warn('Discord Gateway connection closed');
      if (this.running) {
        setTimeout(() => this.connect(), 5000);
      }
    });

    this.ws.on('error', (err) => {
      logger.error('Discord Gateway error:', (err as Error).message);
    });
  }

  private handlePayload(payload: {
    op: number;
    d: unknown;
    s: number | null;
    t: string | null;
  }): void {
    const { op, d, s, t } = payload;
    if (s !== null) this.lastSequence = s;

    switch (op) {
      case 10: {
        // Hello — start heartbeat and identify
        const helloData = d as { heartbeat_interval: number };
        this.startHeartbeat(helloData.heartbeat_interval);
        this.identify();
        break;
      }
      case 11:
        // Heartbeat ACK
        break;
      case 0:
        // Dispatch
        if (t === 'MESSAGE_CREATE') {
          void this.handleMessage(d as DiscordMessagePayload);
        } else if (t === 'INTERACTION_CREATE') {
          void this.handleInteraction(d as DiscordInteraction);
        } else if (t === 'READY') {
          const ready = d as { user: { id: string; username: string } };
          this.botUserId = ready.user.id;
          logger.info(`Discord bot ready as ${ready.user.username} (${ready.user.id})`);
          void this.registerSlashCommands();
        }
        break;
    }
  }

  private startHeartbeat(interval: number): void {
    if (this.heartbeatInterval) clearInterval(this.heartbeatInterval);
    this.heartbeatInterval = setInterval(() => {
      this.sendGateway(1, this.lastSequence);
    }, interval);
  }

  private identify(): void {
    this.sendGateway(2, {
      token: this.config.botToken,
      intents: INTENTS,
      properties: { os: 'linux', browser: 'sera', device: 'sera' },
    });
  }

  private sendGateway(op: number, d: unknown): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ op, d }));
    }
  }

  // ── Message Handling ───────────────────────────────────────────────────────

  private async handleMessage(msg: DiscordMessagePayload): Promise<void> {
    // Ignore bot messages (including our own)
    if (msg.author.bot) return;

    const isDM = !msg.guild_id;
    const isMentioned =
      !isDM &&
      this.botUserId != null &&
      Array.isArray(msg.mentions) &&
      msg.mentions.some((m) => m.id === this.botUserId);

    // Defaults: allowDMs=true, allowMentions=true (must explicitly disable)
    const allowDMs = this.config.allowDMs !== false;
    const allowMentions = this.config.allowMentions !== false;

    if (isDM && !allowDMs) {
      logger.debug(`Ignoring DM from ${msg.author.username} — allowDMs is false`);
      return;
    }
    if (!isDM && !isMentioned) return; // In guilds, only respond to @mentions
    if (!isDM && !allowMentions) {
      logger.debug(`Ignoring mention from ${msg.author.username} — allowMentions is false`);
      return;
    }

    // Security: guild + user allowlists
    if (!this.isAllowed(msg.guild_id, msg.author.id)) {
      logger.warn(
        `Blocked message from user ${msg.author.username} (${msg.author.id}) ` +
          `in guild ${msg.guild_id ?? 'DM'} — not in allowlist`
      );
      return;
    }

    // Strip bot mention from message text in guild channels
    let text = msg.content;
    if (isMentioned && this.botUserId) {
      text = text.replace(new RegExp(`<@!?${this.botUserId}>`, 'g'), '').trim();
    }
    if (!text) return;

    void this.sendTyping(msg.channel_id);

    try {
      const sessionId = await this.getOrCreateSession(msg.author.id, msg.author.username);
      const reply = await this.processChat(msg.author.id, text, sessionId);

      const prefix = this.config.responsePrefix ? `**${this.config.responsePrefix}:** ` : '';
      await this.sendChunked(msg.channel_id, prefix + reply);
    } catch (err) {
      logger.error(`Error processing message from ${msg.author.username}:`, (err as Error).message);
      await this.sendDiscordMessage(
        msg.channel_id,
        '⚠️ An error occurred while processing your message.'
      );
    }
  }

  /**
   * Fetch agent, send message through container chat (with in-process fallback),
   * and persist both turns to the session store. Returns the assistant reply.
   */
  private async processChat(userId: string, text: string, sessionId: string): Promise<string> {
    const targetAgentId = this.getTargetAgent(userId);

    let agent = this.orchestrator.getAgent(targetAgentId);
    if (!agent) {
      agent = await this.orchestrator.startInstance(targetAgentId);
    }

    const messages = await this.sessionStore.getMessages(sessionId);
    const history: ChatMessage[] = messages.map((m) => ({
      role: m.role as ChatMessage['role'],
      content: m.content,
    }));

    let reply: string;
    try {
      const chatUrl = await this.orchestrator.ensureContainerRunning(targetAgentId);
      const chatRes = await fetch(`${chatUrl}/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: text, sessionId, history }),
        signal: AbortSignal.timeout(120_000),
      });
      if (chatRes.ok) {
        const body = (await chatRes.json()) as { result: string | null; error?: string };
        reply = body.result || 'No response generated.';
      } else {
        throw new Error(`Container chat returned ${chatRes.status}`);
      }
    } catch (containerErr) {
      logger.warn('Container chat failed, falling back to in-process:', containerErr);
      const response = await agent.process(text, history);
      reply = response.finalAnswer || response.thought || 'No response generated.';
    }

    await this.sessionStore.addMessage({ sessionId, role: 'user', content: text });
    await this.sessionStore.addMessage({ sessionId, role: 'assistant', content: reply });

    return reply;
  }

  // ── Security ───────────────────────────────────────────────────────────────

  private isAllowed(guildId: string | undefined, userId: string): boolean {
    // Check user allowlist
    if (this.config.allowedUsers && this.config.allowedUsers.length > 0) {
      if (!this.config.allowedUsers.includes(userId)) {
        return false;
      }
    }

    // Check guild allowlist (only for guild messages)
    if (guildId && this.config.allowedGuilds && this.config.allowedGuilds.length > 0) {
      if (!this.config.allowedGuilds.includes(guildId)) {
        return false;
      }
    }

    return true;
  }

  // ── Session Management ─────────────────────────────────────────────────────

  /**
   * Get or create a session for a Discord user.
   * Uses a deterministic ID so the same Discord user always gets the same
   * session with this agent, without needing a separate lookup table.
   */
  private async getOrCreateSession(userId: string, userName: string): Promise<string> {
    const targetAgentId = this.getTargetAgent(userId);
    const key = `discord:${userId}:${targetAgentId}`;

    // Check in-memory cache first
    const cached = this.userSessions.get(key);
    if (cached) return cached;

    // Use a deterministic UUID v5-style ID from the key
    // (simple hash → hex → UUID format for readability)
    const deterministicId = this.hashToUuid(key);

    // Try to fetch existing session
    const existing = await this.sessionStore.getSession(deterministicId);
    if (existing) {
      this.userSessions.set(key, deterministicId);
      return deterministicId;
    }

    // Create new session with the deterministic ID
    const session = await this.sessionStore.createSession({
      id: deterministicId,
      agentName: targetAgentId,
      agentInstanceId: targetAgentId,
      title: `Discord: ${userName}`,
    });

    this.userSessions.set(key, session.id);
    return session.id;
  }

  /** Convert a string key to a deterministic UUID-format string. */
  private hashToUuid(input: string): string {
    const hash = crypto.createHash('sha256').update(input).digest('hex');
    // Format as UUID: 8-4-4-4-12
    return [
      hash.substring(0, 8),
      hash.substring(8, 12),
      hash.substring(12, 16),
      hash.substring(16, 20),
      hash.substring(20, 32),
    ].join('-');
  }

  // ── Slash Commands ─────────────────────────────────────────────────────────

  /**
   * Register slash commands globally via Discord's bulk overwrite endpoint.
   * Called once on READY. Discord caches these — re-registering is idempotent.
   */
  private async registerSlashCommands(): Promise<void> {
    const appId = this.config.applicationId;
    if (!appId) {
      logger.warn('No applicationId configured — slash commands will not be registered');
      return;
    }

    try {
      const resp = await fetch(`${DISCORD_API}/applications/${appId}/commands`, {
        method: 'PUT',
        headers: {
          Authorization: `Bot ${this.config.botToken}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(SLASH_COMMANDS),
      });

      if (resp.ok) {
        logger.info(`Registered ${SLASH_COMMANDS.length} slash commands for application ${appId}`);
      } else {
        const body = await resp.text();
        logger.error(`Failed to register slash commands: ${resp.status} ${body}`);
      }
    } catch (err) {
      logger.error('Error registering slash commands:', (err as Error).message);
    }
  }

  // ── Interaction Handling ──────────────────────────────────────────────────

  private async handleInteraction(interaction: DiscordInteraction): Promise<void> {
    // Only handle application commands (type 2)
    if (interaction.type !== 2) return;

    const user = interaction.member?.user ?? interaction.user;
    if (!user) {
      logger.warn('Interaction with no user info — ignoring');
      return;
    }

    // Security: guild + user allowlists
    if (!this.isAllowed(interaction.guild_id, user.id)) {
      await this.respondToInteraction(interaction.id, interaction.token, 4, {
        content: '⚠️ You are not authorized to use this bot.',
        flags: 64, // EPHEMERAL
      });
      return;
    }

    const commandName = interaction.data?.name;
    switch (commandName) {
      case 'ask':
        await this.handleAskCommand(interaction, user);
        break;
      case 'status':
        await this.handleStatusCommand(interaction, user);
        break;
      case 'history':
        await this.handleHistoryCommand(interaction, user);
        break;
      case 'reset':
        await this.handleResetCommand(interaction, user);
        break;
      case 'agents':
        await this.handleAgentsCommand(interaction);
        break;
      case 'switch':
        await this.handleSwitchCommand(interaction, user);
        break;
      case 'help':
        await this.handleHelpCommand(interaction);
        break;
      default:
        await this.respondToInteraction(interaction.id, interaction.token, 4, {
          content: `Unknown command: ${commandName ?? '(none)'}`,
          flags: 64,
        });
    }
  }

  private async handleAskCommand(
    interaction: DiscordInteraction,
    user: { id: string; username: string }
  ): Promise<void> {
    const message = interaction.data?.options?.find((o) => o.name === 'message')?.value;
    if (typeof message !== 'string' || !message.trim()) {
      await this.respondToInteraction(interaction.id, interaction.token, 4, {
        content: 'Please provide a message.',
        flags: 64,
      });
      return;
    }

    // ACK with deferred response (type 5) — "bot is thinking"
    await this.respondToInteraction(interaction.id, interaction.token, 5);

    try {
      const sessionId = await this.getOrCreateSession(user.id, user.username);
      const reply = await this.processChat(user.id, message.trim(), sessionId);

      const prefix = this.config.responsePrefix ? `**${this.config.responsePrefix}:** ` : '';
      const fullReply = prefix + reply;
      const truncated =
        fullReply.length > MAX_MESSAGE_LENGTH
          ? fullReply.substring(0, MAX_MESSAGE_LENGTH - 3) + '...'
          : fullReply;
      await this.editInteractionResponse(interaction.token, truncated);
    } catch (err) {
      logger.error(`Error processing /ask from ${user.username}:`, (err as Error).message);
      await this.editInteractionResponse(
        interaction.token,
        '⚠️ An error occurred while processing your message.'
      );
    }
  }

  private async handleStatusCommand(
    interaction: DiscordInteraction,
    user: { id: string; username: string }
  ): Promise<void> {
    const targetAgentId = this.getTargetAgent(user.id);
    const agent = this.orchestrator.getAgent(targetAgentId);
    const status = agent ? 'running' : 'stopped';

    const key = `discord:${user.id}:${targetAgentId}`;
    const sessionId = this.userSessions.get(key);

    let messageCount = 0;
    if (sessionId) {
      const messages = await this.sessionStore.getMessages(sessionId);
      messageCount = messages.length;
    }

    await this.respondToInteraction(interaction.id, interaction.token, 4, {
      embeds: [
        {
          title: 'Agent Status',
          color: status === 'running' ? 0x00ff00 : 0xff0000,
          fields: [
            { name: 'Agent', value: targetAgentId, inline: true },
            { name: 'Status', value: status, inline: true },
            {
              name: 'Session',
              value: sessionId ? `\`${sessionId.substring(0, 8)}...\`` : 'none',
              inline: true,
            },
            { name: 'Messages', value: String(messageCount), inline: true },
          ],
          footer: { text: 'SERA' },
        },
      ],
      flags: 64,
    });
  }

  private async handleHistoryCommand(
    interaction: DiscordInteraction,
    user: { id: string; username: string }
  ): Promise<void> {
    const countOpt = interaction.data?.options?.find((o) => o.name === 'count')?.value;
    const count = typeof countOpt === 'number' ? Math.min(Math.max(countOpt, 1), 50) : 10;

    // ACK with deferred response
    await this.respondToInteraction(interaction.id, interaction.token, 5, { flags: 64 });

    const targetAgentId = this.getTargetAgent(user.id);
    const key = `discord:${user.id}:${targetAgentId}`;
    const sessionId = this.userSessions.get(key);

    if (!sessionId) {
      await this.editInteractionResponse(
        interaction.token,
        'No conversation history yet. Use `/ask` to start chatting.'
      );
      return;
    }

    const messages = await this.sessionStore.getMessages(sessionId);
    const recent = messages.slice(-count);

    if (recent.length === 0) {
      await this.editInteractionResponse(interaction.token, 'No messages in your current session.');
      return;
    }

    const formatted = recent
      .map((m, i) => {
        const role = m.role === 'user' ? '👤' : '🤖';
        const content = m.content.length > 100 ? m.content.substring(0, 100) + '...' : m.content;
        return `${i + 1}. ${role} ${content}`;
      })
      .join('\n');

    const header = `**Last ${recent.length} message(s):**\n`;
    const full = header + formatted;
    const truncated =
      full.length > MAX_MESSAGE_LENGTH ? full.substring(0, MAX_MESSAGE_LENGTH - 3) + '...' : full;

    await this.editInteractionResponse(interaction.token, truncated);
  }

  private async handleResetCommand(
    interaction: DiscordInteraction,
    user: { id: string; username: string }
  ): Promise<void> {
    const targetAgentId = this.getTargetAgent(user.id);
    const key = `discord:${user.id}:${targetAgentId}`;

    // Delete the existing session from the store so getOrCreateSession creates a fresh one
    const oldSessionId = this.userSessions.get(key);
    if (oldSessionId) {
      await this.sessionStore.deleteSession(oldSessionId);
    }
    this.userSessions.delete(key);

    // Create a fresh session (deterministic ID will produce the same key, but the
    // store entry was deleted above so a new empty session is created)
    const newSessionId = await this.getOrCreateSession(user.id, user.username);

    await this.respondToInteraction(interaction.id, interaction.token, 4, {
      content: `✅ Session reset. New session: \`${newSessionId.substring(0, 8)}...\`\nUse \`/ask\` to start a new conversation.`,
      flags: 64,
    });
  }

  private async handleAgentsCommand(interaction: DiscordInteraction): Promise<void> {
    const agents = this.orchestrator.listAgents();

    if (agents.length === 0) {
      await this.respondToInteraction(interaction.id, interaction.token, 4, {
        content: 'No agents are currently registered.',
        flags: 64,
      });
      return;
    }

    const fields = agents.map((a) => ({
      name: a.name,
      value: `**Status:** ${a.status}\n**ID:** \`${a.id || 'n/a'}\``,
      inline: true,
    }));

    await this.respondToInteraction(interaction.id, interaction.token, 4, {
      embeds: [
        {
          title: 'Available Agents',
          color: 0x5865f2, // Discord blurple
          fields,
          footer: { text: `${agents.length} agent(s) — SERA` },
        },
      ],
      flags: 64,
    });
  }

  private async handleSwitchCommand(
    interaction: DiscordInteraction,
    user: { id: string; username: string }
  ): Promise<void> {
    const agentOpt = interaction.data?.options?.find((o) => o.name === 'agent')?.value;
    if (typeof agentOpt !== 'string' || !agentOpt.trim()) {
      await this.respondToInteraction(interaction.id, interaction.token, 4, {
        content: 'Please provide an agent name or ID.',
        flags: 64,
      });
      return;
    }

    const agentId = agentOpt.trim();

    // Validate the agent exists (check running agents and manifests)
    const agent = this.orchestrator.getAgent(agentId);
    const agentInfo = this.orchestrator.getAgentInfo(agentId);
    if (!agent && !agentInfo) {
      await this.respondToInteraction(interaction.id, interaction.token, 4, {
        content: `⚠️ Agent \`${agentId}\` not found. Use \`/agents\` to see available agents.`,
        flags: 64,
      });
      return;
    }

    // Clear the old session cache so next /ask creates a fresh session for the new agent
    const oldTargetId = this.getTargetAgent(user.id);
    const oldKey = `discord:${user.id}:${oldTargetId}`;
    this.userSessions.delete(oldKey);

    // Set the per-user override
    this.userTargetAgent.set(user.id, agentId);

    await this.respondToInteraction(interaction.id, interaction.token, 4, {
      embeds: [
        {
          title: 'Agent Switched',
          color: 0x57f287, // Discord green
          fields: [
            { name: 'Now talking to', value: `\`${agentId}\``, inline: true },
            { name: 'Previous', value: `\`${oldTargetId}\``, inline: true },
          ],
          footer: { text: 'Session reset — use /ask to start chatting. — SERA' },
        },
      ],
      flags: 64,
    });
  }

  private async handleHelpCommand(interaction: DiscordInteraction): Promise<void> {
    const commands = SLASH_COMMANDS.map((cmd) => `**/${cmd.name}** — ${cmd.description}`);

    await this.respondToInteraction(interaction.id, interaction.token, 4, {
      embeds: [
        {
          title: 'SERA Commands',
          color: 0x5865f2,
          description: commands.join('\n'),
          footer: { text: 'SERA' },
        },
      ],
      flags: 64,
    });
  }

  // ── Interaction API ───────────────────────────────────────────────────────

  /**
   * Send an initial response to a Discord interaction.
   * @param type 4 = immediate response, 5 = deferred (shows "thinking...")
   */
  private async respondToInteraction(
    interactionId: string,
    interactionToken: string,
    type: number,
    data?: {
      content?: string;
      embeds?: Array<{
        title?: string;
        description?: string;
        color?: number;
        fields?: Array<{ name: string; value: string; inline?: boolean }>;
        footer?: { text: string };
      }>;
      flags?: number;
    }
  ): Promise<void> {
    try {
      const resp = await fetch(
        `${DISCORD_API}/interactions/${interactionId}/${interactionToken}/callback`,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ type, data }),
        }
      );
      if (!resp.ok) {
        logger.error(`Interaction response error: ${resp.status} ${resp.statusText}`);
      }
    } catch (err) {
      logger.error('Failed to respond to interaction:', (err as Error).message);
    }
  }

  /**
   * Edit the original deferred interaction response.
   */
  private async editInteractionResponse(interactionToken: string, content: string): Promise<void> {
    try {
      const resp = await fetch(
        `${DISCORD_API}/webhooks/${this.config.applicationId}/${interactionToken}/messages/@original`,
        {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ content }),
        }
      );
      if (!resp.ok) {
        logger.error(`Edit interaction response error: ${resp.status} ${resp.statusText}`);
      }
    } catch (err) {
      logger.error('Failed to edit interaction response:', (err as Error).message);
    }
  }

  // ── Discord API ────────────────────────────────────────────────────────────

  private async sendDiscordMessage(channelId: string, content: string): Promise<void> {
    try {
      const resp = await fetch(`${DISCORD_API}/channels/${channelId}/messages`, {
        method: 'POST',
        headers: {
          Authorization: `Bot ${this.config.botToken}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ content }),
      });
      if (!resp.ok) {
        logger.error(`Discord API error: ${resp.status} ${resp.statusText}`);
      }
    } catch (err) {
      logger.error(`Failed to send Discord message:`, (err as Error).message);
    }
  }

  private async sendChunked(channelId: string, text: string): Promise<void> {
    if (text.length <= MAX_MESSAGE_LENGTH) {
      await this.sendDiscordMessage(channelId, text);
      return;
    }

    // Split on newlines or sentence boundaries, respecting the limit
    let remaining = text;
    while (remaining.length > 0) {
      let chunk: string;
      if (remaining.length <= MAX_MESSAGE_LENGTH) {
        chunk = remaining;
        remaining = '';
      } else {
        // Find a good split point
        let splitAt = remaining.lastIndexOf('\n', MAX_MESSAGE_LENGTH);
        if (splitAt < MAX_MESSAGE_LENGTH * 0.5) {
          splitAt = remaining.lastIndexOf('. ', MAX_MESSAGE_LENGTH);
        }
        if (splitAt < MAX_MESSAGE_LENGTH * 0.3) {
          splitAt = MAX_MESSAGE_LENGTH;
        }
        chunk = remaining.substring(0, splitAt);
        remaining = remaining.substring(splitAt).trimStart();
      }
      await this.sendDiscordMessage(channelId, chunk);
    }
  }

  private async sendTyping(channelId: string): Promise<void> {
    try {
      await fetch(`${DISCORD_API}/channels/${channelId}/typing`, {
        method: 'POST',
        headers: { Authorization: `Bot ${this.config.botToken}` },
      });
    } catch {
      // Non-critical — ignore typing indicator failures
    }
  }
}
