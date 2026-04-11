/**
 * SERA Discord Bridge
 *
 * Listens for Discord DMs and @mentions, forwards them to the SERA Rust core
 * via POST /api/chat, and replies with the response.
 *
 * Required env vars: DISCORD_TOKEN
 * Optional env vars: SERA_CORE_URL, SERA_API_KEY, SERA_AGENT_ID
 */

import {
  Client,
  Events,
  GatewayIntentBits,
  Message,
  Partials,
} from 'discord.js';

const SERA_CORE_URL = process.env['SERA_CORE_URL'] ?? 'http://localhost:3001';
const SERA_API_KEY = process.env['SERA_API_KEY'] ?? 'sera_bootstrap_dev_123';
const SERA_AGENT_ID =
  process.env['SERA_AGENT_ID'] ?? 'e39b5569-f110-49a2-99c5-25758872a958';
const DISCORD_TOKEN = process.env['DISCORD_TOKEN'];
const MAX_MESSAGE_LENGTH = 2000;

if (!DISCORD_TOKEN) {
  console.error('DISCORD_TOKEN env var is required');
  process.exit(1);
}

interface ChatRequest {
  message: string;
  agentId: string;
  sessionId: string;
}

interface ChatResponse {
  reply?: string;
  response?: string;
  result?: string;
  error?: string;
}

const client = new Client({
  intents: [
    GatewayIntentBits.Guilds,
    GatewayIntentBits.GuildMessages,
    GatewayIntentBits.DirectMessages,
    GatewayIntentBits.MessageContent,
  ],
  partials: [Partials.Channel, Partials.Message],
});

async function forwardToSera(message: string, channelId: string): Promise<string> {
  const body: ChatRequest = {
    message,
    agentId: SERA_AGENT_ID,
    sessionId: `discord-${channelId}`,
  };

  const res = await fetch(`${SERA_CORE_URL}/api/chat`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${SERA_API_KEY}`,
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(120_000),
  });

  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(`SERA API error ${res.status}: ${text}`);
  }

  const data = (await res.json()) as ChatResponse;
  return data.reply ?? data.response ?? data.result ?? 'No response.';
}

async function sendChunked(msg: Message, text: string): Promise<void> {
  if (text.length <= MAX_MESSAGE_LENGTH) {
    await msg.reply(text);
    return;
  }

  let remaining = text;
  let first = true;
  while (remaining.length > 0) {
    let chunk: string;
    if (remaining.length <= MAX_MESSAGE_LENGTH) {
      chunk = remaining;
      remaining = '';
    } else {
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

    if (first) {
      await msg.reply(chunk);
      first = false;
    } else {
      await msg.channel.send(chunk);
    }
  }
}

client.on(Events.MessageCreate, async (msg: Message) => {
  // DMs arrive as partials — fetch full message before processing
  if (msg.partial) {
    try {
      msg = await msg.fetch();
    } catch (err) {
      console.error('Failed to fetch partial message:', err);
      return;
    }
  }

  // Ignore bots (including self)
  if (msg.author.bot) return;

  const isDM = !msg.guild;
  const isMentioned =
    !isDM && client.user != null && msg.mentions.has(client.user);

  if (!isDM && !isMentioned) return;

  // Strip @mention from guild messages
  let content = msg.content;
  if (isMentioned && client.user) {
    content = content.replace(new RegExp(`<@!?${client.user.id}>`, 'g'), '').trim();
  }
  if (!content) return;

  await msg.channel.sendTyping().catch(() => undefined);

  try {
    const reply = await forwardToSera(content, msg.channelId);
    await sendChunked(msg, reply);
  } catch (err) {
    console.error('Error forwarding message to SERA:', err);
    await msg.reply('⚠️ Failed to reach SERA. Please try again.').catch(() => undefined);
  }
});

client.once(Events.ClientReady, (c) => {
  console.log(`Discord bridge ready — logged in as ${c.user.tag}`);
  console.log(`Forwarding to ${SERA_CORE_URL} → agent ${SERA_AGENT_ID}`);
});

client.login(DISCORD_TOKEN).catch((err: unknown) => {
  console.error('Failed to log in to Discord:', err);
  process.exit(1);
});
