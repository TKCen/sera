/**
 * CentrifugoPublisher & CentrifugoSubscriber — lightweight clients for 
 * real-time messaging in the agent runtime.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { Centrifuge } from 'centrifuge';
import { log } from './logger.js';
import { type TokenUsage } from './usage.js';

export interface IntercomMessage {
  id: string;
  version: '1';
  timestamp: string;
  source: { agent: string; circle: string };
  target: { channel: string };
  type: string;
  payload: Record<string, unknown>;
  metadata: {
    replyTo?: string;
    ttl?: number;
    securityTier: number;
  };
}

// ── Types ─────────────────────────────────────────────────────────────────────

export type ThoughtType = 'observe' | 'plan' | 'act' | 'reflect';

/** Extended set used internally (not emitted to the spec thought channel). */
export type InternalStepType = ThoughtType | 'tool-call' | 'tool-result' | 'reasoning';

export interface ThoughtEvent {
  stepType: ThoughtType;
  content: string;
  timestamp: string;
  iteration: number;
  agentId: string;
  agentDisplayName: string;
  toolName?: string;
  toolArgs?: Record<string, unknown>;
  anomaly?: boolean;
}

export interface StreamToken {
  token: string;
  done: boolean;
  messageId: string;
}

export interface UsageEvent {
  agentId: string;
  usage: TokenUsage;
  costUsd: number;
  timestamp: string;
}

// ── Publisher ─────────────────────────────────────────────────────────────────

export class CentrifugoPublisher {
  private http: AxiosInstance;
  private agentId: string;
  private agentDisplayName: string;

  constructor(
    centrifugoUrl: string,
    apiKey: string,
    agentId: string,
    agentDisplayName: string,
  ) {
    this.agentId = agentId;
    this.agentDisplayName = agentDisplayName;
    this.http = axios.create({
      baseURL: centrifugoUrl,
      headers: {
        'Content-Type': 'application/json',
        'X-API-Key': apiKey,
      },
      timeout: 5000,
    });
  }

  async publish(channel: string, data: unknown): Promise<void> {
    try {
      await this.http.post('', {
        method: 'publish',
        params: { channel, data },
      });
    } catch (err) {
      const msg = err instanceof AxiosError ? err.message : String(err);
      log('warn', `Failed to publish to ${channel}: ${msg}`);
    }
  }

  async publishThought(
    step: ThoughtType | InternalStepType,
    content: string,
    iteration: number = 0,
    opts?: {
      toolName?: string;
      toolArgs?: Record<string, unknown>;
      anomaly?: boolean;
    },
  ): Promise<void> {
    const canonical = this.toCanonicalStep(step);
    const channel = `thoughts:${this.agentId}`;

    const event: ThoughtEvent = {
      stepType: canonical,
      content,
      timestamp: new Date().toISOString(),
      iteration,
      agentId: this.agentId,
      agentDisplayName: this.agentDisplayName,
      ...(opts?.toolName !== undefined ? { toolName: opts.toolName } : {}),
      ...(opts?.toolArgs !== undefined ? { toolArgs: opts.toolArgs } : {}),
      ...(opts?.anomaly !== undefined ? { anomaly: opts.anomaly } : {}),
    };

    await this.publish(channel, event);
  }

  async publishStreamToken(
    messageId: string,
    token: string,
    done: boolean,
  ): Promise<void> {
    const channel = `tokens:${this.agentId}`;
    const data: StreamToken = { token, done, messageId };
    await this.publish(channel, data);
  }

  async publishUsage(
    usage: TokenUsage,
    costUsd: number,
  ): Promise<void> {
    const channel = `usage:${this.agentId}`;
    const event: UsageEvent = {
      agentId: this.agentId,
      usage,
      costUsd,
      timestamp: new Date().toISOString(),
    };
    await this.publish(channel, event);
  }

  private toCanonicalStep(step: ThoughtType | InternalStepType): ThoughtType {
    switch (step) {
      case 'observe':
      case 'plan':
      case 'act':
      case 'reflect':
        return step;
      case 'tool-call':
      case 'act':
        return 'act';
      case 'tool-result':
        return 'reflect';
      case 'reasoning':
        return 'observe';
      default:
        return 'observe';
    }
  }
}

// ── Subscriber ────────────────────────────────────────────────────────────────

export class CentrifugoSubscriber {
  private client: Centrifuge;
  private agentId: string;

  constructor(wsUrl: string, token: string, agentId: string) {
    this.agentId = agentId;
    this.client = new Centrifuge(wsUrl, {
      token,
      debug: process.env['NODE_ENV'] === 'development',
    });

    this.client.on('connected', () => log('info', `Subscriber connected to Centrifugo` ));
    this.client.on('error', (ctx) => log('error', `Subscriber error: ${ctx.error.message}`));
    this.client.on('disconnected', () => log('warn', `Subscriber disconnected`));
  }

  async connect(): Promise<void> {
    this.client.connect();
  }

  async disconnect(): Promise<void> {
    this.client.disconnect();
  }

  /**
   * Subscribe to a channel and handle messages.
   */
  async subscribe(channel: string, onMessage: (msg: IntercomMessage) => void): Promise<void> {
    const sub = this.client.newSubscription(channel);
    
    sub.on('publication', (ctx) => {
      try {
        const msg = ctx.data as IntercomMessage;
        onMessage(msg);
      } catch (err) {
        log('error', `Failed to parse intercom message on ${channel}`);
      }
    });

    sub.on('error', (ctx) => log('error', `Subscription error on ${channel}: ${ctx.error.message}`));
    
    sub.subscribe();
    log('info', `Subscribed to ${channel}`);
  }
}
