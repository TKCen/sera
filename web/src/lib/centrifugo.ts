/**
 * Centrifugo client wrapper for the SERA web UI.
 *
 * Provides a singleton connection to the Centrifugo server and
 * helper functions for subscribing to agent thought streams.
 */

import { Centrifuge, type Subscription, type PublicationContext } from 'centrifuge';

// ── Client Singleton ────────────────────────────────────────────────────────────

let client: Centrifuge | null = null;

function getCentrifugoUrl(): string {
  if (typeof window === 'undefined') {
    // Server-side rendering — won't actually connect
    return 'ws://localhost:10001/connection/websocket';
  }
  // Use env var if set, otherwise construct from current host
  const envUrl = process.env['NEXT_PUBLIC_CENTRIFUGO_URL'];
  if (envUrl) return envUrl;
  const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
  return `${proto}://${window.location.hostname}:10001/connection/websocket`;
}

export function getClient(): Centrifuge {
  if (!client) {
    client = new Centrifuge(getCentrifugoUrl(), {
      // No token needed for development (anonymous connections allowed by config)
    });
    client.connect();
  }
  return client;
}

export function disconnectClient(): void {
  if (client) {
    client.disconnect();
    client = null;
  }
}

// ── Thought Stream Subscription ─────────────────────────────────────────────────

export interface ThoughtEvent {
  timestamp: string;
  stepType: 'observe' | 'plan' | 'act' | 'reflect' | 'tool-call' | 'tool-result' | 'reasoning';
  content: string;
  agentId: string;
  agentDisplayName: string;
}

/**
 * Safely get or create a subscription to a channel.
 * centrifuge-js throws if newSubscription is called on an existing channel,
 * so we must remove any previous subscription first.
 */
function safeNewSubscription(centrifuge: Centrifuge, channel: string): Subscription {
  const existing = centrifuge.getSubscription(channel);
  if (existing) {
    existing.unsubscribe();
    existing.removeAllListeners();
    centrifuge.removeSubscription(existing);
  }
  return centrifuge.newSubscription(channel);
}

/**
 * Subscribe to an agent's thought stream.
 * Returns an unsubscribe function.
 */
export function subscribeToThoughts(
  agentId: string,
  onThought: (event: ThoughtEvent) => void,
): () => void {
  const centrifuge = getClient();
  const channel = `internal:agent:${agentId}:thoughts`;

  const sub = safeNewSubscription(centrifuge, channel);

  sub.on('publication', (ctx: PublicationContext) => {
    onThought(ctx.data as ThoughtEvent);
  });

  sub.subscribe();

  return () => {
    sub.unsubscribe();
    sub.removeAllListeners();
    centrifuge.removeSubscription(sub);
  };
}

/**
 * Subscribe to an agent's terminal output stream.
 * Returns an unsubscribe function.
 */
export function subscribeToTerminal(
  agentId: string,
  onOutput: (data: unknown) => void,
): () => void {
  const centrifuge = getClient();
  const channel = `internal:agent:${agentId}:terminal`;

  const sub = safeNewSubscription(centrifuge, channel);

  sub.on('publication', (ctx: PublicationContext) => {
    onOutput(ctx.data);
  });

  sub.subscribe();

  return () => {
    sub.unsubscribe();
    sub.removeAllListeners();
    centrifuge.removeSubscription(sub);
  };
}

// ── Stream Token Subscription ───────────────────────────────────────────────────

export interface StreamToken {
  token: string;
  done: boolean;
  messageId: string;
}

/**
 * Subscribe to a per-message stream channel for real-time token delivery.
 * Subscribe BEFORE triggering the backend to avoid missing tokens.
 * Returns an unsubscribe function.
 */
export function subscribeToStream(
  messageId: string,
  onToken: (token: string) => void,
  onDone: () => void,
): () => void {
  const centrifuge = getClient();
  const channel = `internal:stream:${messageId}`;

  const sub = safeNewSubscription(centrifuge, channel);

  sub.on('publication', (ctx: PublicationContext) => {
    const data = ctx.data as StreamToken;
    if (data.token) {
      onToken(data.token);
    }
    if (data.done) {
      onDone();
      sub.unsubscribe();
      sub.removeAllListeners();
      centrifuge.removeSubscription(sub);
    }
  });

  sub.subscribe();

  return () => {
    sub.unsubscribe();
    sub.removeAllListeners();
    centrifuge.removeSubscription(sub);
  };
}

