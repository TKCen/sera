/**
 * CentrifugoPublisher & CentrifugoSubscriber — lightweight clients for
 * real-time messaging in the agent runtime.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { Centrifuge } from 'centrifuge';
import { log } from './logger.js';
import { safeStringify } from './json.js';

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

export interface ToolOutputEvent {
  toolCallId: string;
  toolName: string;
  type: 'stdout' | 'stderr' | 'progress' | 'result' | 'error';
  content: string;
  done: boolean;
  timestamp: string;
  durationMs?: number;
}

export interface ToolResultEvent {
  toolCallId: string;
  toolName: string;
  result: string; // Truncated result
  duration: number; // Execution time in ms
  error: boolean;
  timestamp: string;
}

export type ToolOutputCallback = (event: ToolOutputEvent | ToolResultEvent) => void;

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
  toolCallId?: string;
  anomaly?: boolean;
  internal?: boolean;
}

export interface StreamToken {
  token: string;
  done: boolean;
  messageId: string;
  error?: string;
}

// ── Publisher ─────────────────────────────────────────────────────────────────

export class CentrifugoPublisher {
  private http: AxiosInstance;
  private agentId: string;
  private agentDisplayName: string;

  constructor(centrifugoUrl: string, apiKey: string, agentId: string, agentDisplayName: string) {
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
      // Use safeStringify to handle cyclic references in tool results/args
      // before axios attempts its own JSON.stringify internally.
      const body = safeStringify({ method: 'publish', params: { channel, data } });
      await this.http.post('', body, {
        headers: { 'Content-Type': 'application/json' },
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
      toolCallId?: string;
      anomaly?: boolean;

      internal?: boolean;
    }
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
      ...(opts?.toolCallId !== undefined ? { toolCallId: opts.toolCallId } : {}),
      ...(opts?.anomaly !== undefined ? { anomaly: opts.anomaly } : {}),
      ...(opts?.internal !== undefined ? { internal: opts.internal } : {}),
    };

    await this.publish(channel, event);
  }

  async publishStreamToken(messageId: string, token: string, done: boolean): Promise<void> {
    const channel = `tokens:${this.agentId}`;
    const data: StreamToken = { token, done, messageId };
    await this.publish(channel, data);
  }

  async publishToolOutput(
    event: ToolOutputEvent | ToolResultEvent,
    messageId?: string
  ): Promise<void> {
    const channel = `tooloutput:${messageId || this.agentId}`;
    await this.publish(channel, event);
  }

  /** Publish an error completion signal so the web UI can stop the spinner. */
  async publishStreamError(messageId: string, errorMessage: string): Promise<void> {
    const channel = `tokens:${this.agentId}`;
    const data: StreamToken = { token: '', done: true, messageId, error: errorMessage };
    await this.publish(channel, data);
  }

  private toCanonicalStep(step: ThoughtType | InternalStepType): ThoughtType {
    switch (step) {
      case 'observe':
      case 'plan':
      case 'act':
      case 'reflect':
        return step;
      case 'tool-call':
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

    this.client.on('connected', () => log('info', `Subscriber connected to Centrifugo`));
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

    sub.on('error', (ctx) =>
      log('error', `Subscription error on ${channel}: ${ctx.error.message}`)
    );

    sub.subscribe();
    log('info', `Subscribed to ${channel}`);
  }
}
