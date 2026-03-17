/**
 * IntercomService — wraps the Centrifugo Server HTTP API to provide
 * real-time messaging between agents, circles, and the web UI.
 *
 * All communication flows through Centrifugo channels using the
 * structured IntercomMessage envelope.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { v4 as uuidv4 } from 'uuid';
import { Logger } from '../lib/logger.js';
import { ChannelNamespace } from './ChannelNamespace.js';
import type {
  IntercomMessage,
  IntercomMessageType,
  MessageMetadata,
  ThoughtEvent,
  ThoughtStepType,
  StreamToken,
} from './types.js';
import type { AgentManifest } from '../agents/manifest/types.js';
import type { BridgeService } from './BridgeService.js';

// ── Configuration ───────────────────────────────────────────────────────────────

const CENTRIFUGO_API_URL = process.env['CENTRIFUGO_API_URL'] || 'http://centrifugo:8000/api';
const CENTRIFUGO_API_KEY = process.env['CENTRIFUGO_API_KEY'] || 'sera-api-key';

// ── Service ─────────────────────────────────────────────────────────────────────

const logger = new Logger('Intercom');

export class IntercomService {
  private readonly http: AxiosInstance;
  private bridgeService?: BridgeService;

  constructor(apiUrl?: string, apiKey?: string) {
    this.http = axios.create({
      baseURL: apiUrl ?? CENTRIFUGO_API_URL,
      headers: {
        'Content-Type': 'application/json',
        'X-API-Key': apiKey ?? CENTRIFUGO_API_KEY,
      },
      timeout: 5000,
    });
  }

  // ── Low-level Centrifugo API ────────────────────────────────────────────────

  /**
   * Hook up the bridge service for federation.
   */
  setBridgeService(bridgeService: BridgeService): void {
    this.bridgeService = bridgeService;
  }

  /**
   * Publish data to a Centrifugo channel.
   */
  async publish(channel: string, data: unknown): Promise<void> {
    try {
      await this.http.post('', {
        method: 'publish',
        params: { channel, data },
      });

      // If it's a bridge channel, forward it via the bridge service
      if (this.bridgeService && typeof data === 'object' && data !== null) {
        const msg = data as IntercomMessage;
        if (msg.id && msg.source && msg.target) {
          await this.bridgeService.handleLocalPublish(channel, msg);
        }
      }
    } catch (err) {
      const message = err instanceof AxiosError ? err.message : String(err);
      logger.error(`Failed to publish to ${channel}: ${message}`);
      throw new IntercomError(`Publish failed: ${message}`, channel);
    }
  }

  /**
   * Check who is subscribed to a Centrifugo channel.
   */
  async presence(channel: string): Promise<Record<string, unknown>> {
    try {
      const res = await this.http.post('', {
        method: 'presence',
        params: { channel },
      });
      return res.data?.result?.presence || {};
    } catch (err) {
      const message = err instanceof AxiosError ? err.message : String(err);
      logger.error(`Failed to get presence for ${channel}: ${message}`);
      return {};
    }
  }

  /**
   * Retrieve channel history from Centrifugo.
   */
  async getHistory(channel: string, limit: number = 50): Promise<unknown[]> {
    try {
      const res = await this.http.post('', {
        method: 'history',
        params: { channel, limit },
      });
      const publications = res.data?.result?.publications;
      return Array.isArray(publications) ? publications.map((p: any) => p.data) : [];
    } catch (err) {
      const message = err instanceof AxiosError ? err.message : String(err);
      logger.error(`Failed to get history for ${channel}: ${message}`);
      return [];
    }
  }

  // ── Structured Publishing ───────────────────────────────────────────────────

  /**
   * Build and publish a standard IntercomMessage envelope.
   */
  async publishMessage(
    sourceAgent: string,
    sourceCircle: string,
    channel: string,
    type: IntercomMessageType,
    payload: Record<string, unknown>,
    metadata?: { replyTo?: string; ttl?: number; securityTier?: number },
  ): Promise<IntercomMessage> {
    const msgMetadata: MessageMetadata = {
      securityTier: metadata?.securityTier ?? 1,
    };
    if (metadata?.replyTo !== undefined) msgMetadata.replyTo = metadata.replyTo;
    if (metadata?.ttl !== undefined) msgMetadata.ttl = metadata.ttl;

    const msg: IntercomMessage = {
      id: uuidv4(),
      timestamp: new Date().toISOString(),
      source: { agent: sourceAgent, circle: sourceCircle },
      target: { channel },
      type,
      payload,
      metadata: msgMetadata,
    };

    await this.publish(channel, msg);
    return msg;
  }

  // ── Agent-to-Agent Messaging ────────────────────────────────────────────────

  /**
   * Send a direct message between two agents.
   * Validates that the sender is allowed to message the recipient.
   */
  async sendDirectMessage(
    fromManifest: AgentManifest,
    toAgent: string,
    payload: Record<string, unknown>,
  ): Promise<IntercomMessage> {
    const fromAgent = fromManifest.metadata.name;
    const circle = fromManifest.metadata.circle;

    // Support remote addressing: agent@circle@instance or agent@circle
    if (toAgent.includes('@')) {
      const parts = toAgent.split('@');
      const remoteAgent = parts[0];
      const remoteCircle = parts[1];
      // Note: we don't strictly use the instance part in the channel name,
      // the bridge service uses the circle name to find the connection.

      const channel = ChannelNamespace.bridgeDm(circle, remoteCircle, fromAgent, remoteAgent);
      return this.publishMessage(fromAgent, circle, channel, 'message', payload, {
        securityTier: fromManifest.metadata.tier,
      });
    }

    // Validate permission for local DMs
    const canMessage = fromManifest.intercom?.canMessage ?? [];
    if (!canMessage.includes(toAgent)) {
      throw new IntercomPermissionError(fromAgent, toAgent);
    }

    const channel = ChannelNamespace.dm(circle, fromAgent, toAgent);

    return this.publishMessage(fromAgent, circle, channel, 'message', payload, {
      securityTier: fromManifest.metadata.tier,
    });
  }

  // ── Thought Streaming ──────────────────────────────────────────────────────

  /**
   * Publish a thought/reasoning step to the agent's thoughts channel.
   * The web UI subscribes to this for real-time observability.
   */
  async publishThought(
    agentId: string,
    agentDisplayName: string,
    stepType: ThoughtStepType,
    content: string,
  ): Promise<void> {
    const channel = ChannelNamespace.thoughts(agentId);
    const event: ThoughtEvent = {
      timestamp: new Date().toISOString(),
      stepType,
      content,
      agentId,
      agentDisplayName,
    };

    await this.publish(channel, event);
  }

  // ── Token Streaming ────────────────────────────────────────────────────────

  /**
   * Publish a stream token to a per-message channel.
   * The frontend subscribes before triggering the backend so it catches every token.
   */
  async publishStreamToken(
    channel: string,
    token: string,
    done: boolean,
    messageId: string,
  ): Promise<void> {
    const data: StreamToken = { token, done, messageId };
    await this.publish(channel, data);
  }

  // ── Circle Channel Publishing ──────────────────────────────────────────────

  /**
   * Publish to a circle-scoped channel.
   * Validates the agent subscribes or publishes to this channel.
   */
  async publishToCircleChannel(
    manifest: AgentManifest,
    channelName: string,
    payload: Record<string, unknown>,
  ): Promise<IntercomMessage> {
    const publishChannels = manifest.intercom?.channels?.publish ?? [];
    if (!publishChannels.includes(channelName)) {
      throw new IntercomError(
        `Agent "${manifest.metadata.name}" is not permitted to publish to channel "${channelName}"`,
        channelName,
      );
    }

    const channel = ChannelNamespace.circleChannel(manifest.metadata.circle, channelName);
    return this.publishMessage(
      manifest.metadata.name,
      manifest.metadata.circle,
      channel,
      'message',
      payload,
      { securityTier: manifest.metadata.tier },
    );
  }

  /**
   * Get the list of channels an agent is allowed to interact with,
   * based on its manifest configuration.
   */
  getAgentChannels(manifest: AgentManifest): {
    thoughts: string;
    terminal: string;
    publishChannels: string[];
    subscribeChannels: string[];
    dmPeers: string[];
  } {
    const agentId = manifest.metadata.name;
    const circle = manifest.metadata.circle;

    const dmPeers = (manifest.intercom?.canMessage ?? []).map(peer => {
      if (peer.includes('@')) {
        const parts = peer.split('@');
        return ChannelNamespace.bridgeDm(circle, parts[1], agentId, parts[0]);
      }
      return ChannelNamespace.dm(circle, agentId, peer);
    });

    return {
      thoughts: ChannelNamespace.thoughts(agentId),
      terminal: ChannelNamespace.terminal(agentId),
      publishChannels: (manifest.intercom?.channels?.publish ?? []).map(
        name => ChannelNamespace.circleChannel(circle, name),
      ),
      subscribeChannels: (manifest.intercom?.channels?.subscribe ?? []).map(
        name => ChannelNamespace.circleChannel(circle, name),
      ),
      dmPeers,
    };
  }
}

// ── Errors ────────────────────────────────────────────────────────────────────

export class IntercomError extends Error {
  public readonly channel: string;

  constructor(message: string, channel: string) {
    super(message);
    this.name = 'IntercomError';
    this.channel = channel;
  }
}

export class IntercomPermissionError extends IntercomError {
  constructor(fromAgent: string, toAgent: string) {
    super(
      `Agent "${fromAgent}" is not permitted to message "${toAgent}". ` +
      `Add "${toAgent}" to the intercom.canMessage list in ${fromAgent}'s AGENT.yaml.`,
      `dm:${fromAgent}:${toAgent}`,
    );
    this.name = 'IntercomPermissionError';
  }
}
