/**
 * CentrifugoPublisher — lightweight HTTP client for publishing
 * agent thoughts and stream tokens to Centrifugo.
 *
 * Uses the same channel namespace conventions as Core's ChannelNamespace.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { log } from './logger.js';

// ── Types ────────────────────────────────────────────────────────────────────

export type ThoughtStepType = 'observe' | 'plan' | 'act' | 'reflect' | 'tool-call' | 'tool-result' | 'reasoning';

export interface ThoughtEvent {
  timestamp: string;
  stepType: ThoughtStepType;
  content: string;
  agentId: string;
  agentDisplayName: string;
}

export interface StreamToken {
  token: string;
  done: boolean;
  messageId: string;
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

  /**
   * Publish raw data to a Centrifugo channel.
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
      // Non-fatal — don't crash the runtime
    }
  }

  /**
   * Publish a reasoning thought step.
   */
  async publishThought(stepType: ThoughtStepType, content: string): Promise<void> {
    const channel = `internal:agent:${this.agentId}:thoughts`;
    const event: ThoughtEvent = {
      timestamp: new Date().toISOString(),
      stepType,
      content,
      agentId: this.agentId,
      agentDisplayName: this.agentDisplayName,
    };

    await this.publish(channel, event);
  }

  /**
   * Publish a stream token to a per-message channel.
   */
  async publishStreamToken(
    messageId: string,
    token: string,
    done: boolean,
  ): Promise<void> {
    const channel = `internal:stream:${messageId}`;
    const data: StreamToken = { token, done, messageId };
    await this.publish(channel, data);
  }
}
