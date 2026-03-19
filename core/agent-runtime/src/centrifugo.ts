/**
 * CentrifugoPublisher — lightweight HTTP client for publishing
 * agent thoughts and stream tokens to Centrifugo.
 *
 * Uses the same channel namespace conventions as Core's ChannelNamespace.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { log } from './logger.js';

// ── Types ─────────────────────────────────────────────────────────────────────

export type ThoughtType = 'observe' | 'plan' | 'act' | 'reflect';

/** Extended set used internally (not emitted to the spec thought channel). */
export type InternalStepType = ThoughtType | 'tool-call' | 'tool-result' | 'reasoning';

export interface ThoughtEvent {
  /** Canonical thought type per Story 5.5. */
  step: ThoughtType;
  content: string;
  timestamp: string;
  iteration: number;
  agentId: string;
  agentName: string;
  /** Present only for act thoughts. */
  toolName?: string;
  /** Present only for act thoughts. */
  toolArgs?: Record<string, unknown>;
  /** Anomaly flag per Story 5.10. */
  anomaly?: boolean;
}

export interface StreamToken {
  token: string;
  done: boolean;
  messageId: string;
}

// ── Publisher ─────────────────────────────────────────────────────────────────

export class CentrifugoPublisher {
  private http: AxiosInstance;
  /** Agent instance UUID — used in channel names. */
  private agentId: string;
  /** Agent display/template name — used in channel names. */
  private agentName: string;

  constructor(
    centrifugoUrl: string,
    apiKey: string,
    agentId: string,
    agentName: string,
  ) {
    this.agentId = agentId;
    this.agentName = agentName;
    this.http = axios.create({
      baseURL: centrifugoUrl,
      headers: {
        'Content-Type': 'application/json',
        'X-API-Key': apiKey,
      },
      timeout: 5000,
    });
  }

  /**
   * Publish raw data to a Centrifugo channel.
   * Best-effort — failures are logged but do not propagate.
   */
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

  /**
   * Publish a canonical reasoning thought step.
   * Channel: `thoughts:{agentId}:{agentName}`
   */
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
    // Map internal step types to canonical thought types
    const canonical = this.toCanonicalStep(step);
    const channel = `thoughts:${this.agentId}:${this.agentName}`;

    const event: ThoughtEvent = {
      step: canonical,
      content,
      timestamp: new Date().toISOString(),
      iteration,
      agentId: this.agentId,
      agentName: this.agentName,
      ...(opts?.toolName !== undefined ? { toolName: opts.toolName } : {}),
      ...(opts?.toolArgs !== undefined ? { toolArgs: opts.toolArgs } : {}),
      ...(opts?.anomaly !== undefined ? { anomaly: opts.anomaly } : {}),
    };

    await this.publish(channel, event);
  }

  /**
   * Publish an LLM stream token.
   * Channel: `tokens:{agentId}`
   */
  async publishStreamToken(
    messageId: string,
    token: string,
    done: boolean,
  ): Promise<void> {
    const channel = `tokens:${this.agentId}`;
    const data: StreamToken = { token, done, messageId };
    await this.publish(channel, data);
  }

  // ── Helpers ───────────────────────────────────────────────────────────────

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
