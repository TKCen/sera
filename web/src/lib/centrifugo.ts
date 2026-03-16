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
  // Use the env var configured in docker-compose / next.config
  return (
    process.env['NEXT_PUBLIC_CENTRIFUGO_URL'] ||
    'ws://localhost:10001/connection/websocket'
  );
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
  stepType: 'observe' | 'plan' | 'act' | 'reflect';
  content: string;
  agentId: string;
  agentDisplayName: string;
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

  const sub: Subscription = centrifuge.newSubscription(channel);

  sub.on('publication', (ctx: PublicationContext) => {
    onThought(ctx.data as ThoughtEvent);
  });

  sub.subscribe();

  return () => {
    sub.unsubscribe();
    sub.removeAllListeners();
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

  const sub: Subscription = centrifuge.newSubscription(channel);

  sub.on('publication', (ctx: PublicationContext) => {
    onOutput(ctx.data);
  });

  sub.subscribe();

  return () => {
    sub.unsubscribe();
    sub.removeAllListeners();
  };
}
