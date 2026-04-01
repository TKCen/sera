/**
 * IntercomService — wraps the Centrifugo Server HTTP API to provide
 * real-time messaging between agents, circles, and the web UI.
 *
 * All communication flows through Centrifugo channels using the
 * structured IntercomMessage envelope.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { v4 as uuidv4 } from 'uuid';
import * as jose from 'jose';
import { Logger } from '../lib/logger.js';
import { ChannelNamespace } from './ChannelNamespace.js';
import { pool } from '../lib/database.js';
import type { BridgeService } from './BridgeService.js';
import type {
  IntercomMessage,
  IntercomMessageType,
  MessageMetadata,
  ThoughtEvent,
  ThoughtStepType,
  StreamToken,
} from './types.js';
import type { AgentManifest } from '../agents/index.js';

// ── Configuration ───────────────────────────────────────────────────────────────

const CENTRIFUGO_API_URL = process.env['CENTRIFUGO_API_URL'] || 'http://centrifugo:8000/api';
const CENTRIFUGO_API_KEY = process.env['CENTRIFUGO_API_KEY'] || 'sera-api-key';
const CENTRIFUGO_TOKEN_SECRET = process.env['CENTRIFUGO_TOKEN_SECRET'] || 'sera-token-secret';

// ── Service ─────────────────────────────────────────────────────────────────────

const logger = new Logger('Intercom');

export class IntercomService {
  private readonly http: AxiosInstance;

  private bridge?: BridgeService;

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

  /**
   * Set the bridge service for cross-instance federation.
   */
  setBridgeService(bridge: BridgeService): void {
    this.bridge = bridge;
  }

  // ── Low-level Centrifugo API ────────────────────────────────────────────────

  /**
   * Publish data to a Centrifugo channel.
   */
  async publish(channel: string, data: unknown): Promise<void> {
    try {
      await this.http.post('', {
        method: 'publish',
        params: { channel, data },
      });

      // Forward to bridge for federation
      if (this.bridge && data && typeof data === 'object') {
        this.bridge.handleLocalPublish(channel, data as IntercomMessage).catch((err: unknown) => {
          logger.warn(`Bridge failed to forward ${channel}: ${(err as Error).message}`);
        });
      }
    } catch (err) {
      const message = err instanceof AxiosError ? err.message : String(err);
      logger.warn(`Failed to publish to ${channel}: ${message}`);
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
      return Array.isArray(publications)
        ? publications.map((p: Record<string, unknown>) => p.data)
        : [];
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
    sourceAgentId: string,
    sourceCircleId: string,
    channel: string,
    type: IntercomMessageType,
    payload: Record<string, unknown>,
    metadata?: { replyTo?: string; ttl?: number; securityTier?: number }
  ): Promise<IntercomMessage> {
    const msgMetadata: MessageMetadata = {
      securityTier: metadata?.securityTier ?? 1,
    };
    if (metadata?.replyTo !== undefined) msgMetadata.replyTo = metadata.replyTo;
    if (metadata?.ttl !== undefined) msgMetadata.ttl = metadata.ttl;

    const msg: IntercomMessage = {
      id: uuidv4(),
      version: '1',
      timestamp: new Date().toISOString(),
      source: { agent: sourceAgentId, circle: sourceCircleId },
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
    toAgentId: string,
    payload: Record<string, unknown>
  ): Promise<IntercomMessage> {
    const fromAgentId = fromManifest.metadata.name;
    const fromCircle = fromManifest.metadata.circle || 'unknown';

    // Validate permission
    const canMessage = fromManifest.intercom?.canMessage ?? [];
    if (!canMessage.includes(toAgentId) && !canMessage.includes('*')) {
      throw new IntercomPermissionError(fromAgentId, toAgentId);
    }

    let channel: string;
    if (toAgentId.includes('@')) {
      // Remote agent: agent-b@circle-b
      const [targetAgent, targetCircle] = toAgentId.split('@');
      if (!targetAgent || !targetCircle) {
        throw new Error(`Invalid remote agent identifier: ${toAgentId}`);
      }
      channel = ChannelNamespace.bridgeDm(fromCircle, targetCircle, fromAgentId, targetAgent);
    } else {
      channel = ChannelNamespace.private(fromAgentId, toAgentId);
    }

    return this.publishMessage(fromAgentId, fromCircle, channel, 'message', payload, {
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
    taskId?: string,
    iteration?: number
  ): Promise<void> {
    const channel = ChannelNamespace.thoughts(agentId);
    const timestamp = new Date().toISOString();
    const event: ThoughtEvent = {
      timestamp,
      stepType,
      content,
      agentId,
      agentDisplayName,
      taskId,
      iteration,
    };

    // Story 9.7: Persist thought to database (non-blocking).
    // Skip persistence for YAML-loaded agents that use their manifest name (not a UUID)
    // as their agentId — inserting a non-UUID value into the uuid column crashes the query.
    const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
    const persistThought = async () => {
      if (!UUID_RE.test(agentId)) return; // not a DB-registered instance — skip
      try {
        await pool.query(
          `INSERT INTO thought_events (agent_instance_id, task_id, step, content, iteration, published_at)
           VALUES ($1, $2, $3, $4, $5, $6)`,
          [agentId, taskId || null, stepType, content, iteration || 0, timestamp]
        );
      } catch (err: unknown) {
        logger.error(`Failed to persist thought for ${agentId}: ${(err as Error).message}`);
      }
    };

    // Fire and forget
    persistThought();

    await this.publish(channel, event);
  }

  /**
   * Story 9.7: Get persisted thoughts for an agent.
   */
  async getThoughts(
    agentId: string,
    options: { taskId?: string; limit?: number; offset?: number } = {}
  ): Promise<ThoughtEvent[]> {
    const { taskId, limit = 50, offset = 0 } = options;
    let queryText = `SELECT * FROM thought_events WHERE agent_instance_id = $1`;
    const params: unknown[] = [agentId];

    if (taskId) {
      params.push(taskId);
      queryText += ` AND task_id = $${params.length}`;
    }

    queryText += ` ORDER BY published_at DESC LIMIT $${params.length + 1} OFFSET $${params.length + 2}`;
    params.push(limit, offset);

    const res = await pool.query(queryText, params);
    return res.rows.map((row) => ({
      timestamp: row.published_at,
      stepType: row.step,
      content: row.content,
      agentId: row.agent_instance_id,
      agentDisplayName: '', // Display name not in DB for now
      taskId: row.task_id,
      iteration: row.iteration,
    }));
  }

  // ── Token Streaming ────────────────────────────────────────────────────────

  /**
   * Publish an LLM stream token delta.
   */
  async publishToken(
    agentId: string,
    token: string,
    done: boolean,
    messageId: string
  ): Promise<void> {
    const channel = ChannelNamespace.tokens(agentId);
    const data: StreamToken = { token, done, messageId };
    await this.publish(channel, data);
  }

  // ── Status Publishing ──────────────────────────────────────────────────────

  /**
   * Publish agent lifecycle status transition.
   */
  async publishAgentStatus(agentId: string, status: string): Promise<void> {
    const channel = ChannelNamespace.status(agentId);
    await this.publish(channel, {
      timestamp: new Date().toISOString(),
      agentId,
      status,
    });
  }

  // ── System Events ──────────────────────────────────────────────────────────

  /**
   * Publish a platform-level system event.
   */
  async publishSystem(event: string, payload: Record<string, unknown>): Promise<void> {
    await this.publishSystemEvent(event, payload);
  }

  /**
   * Publish a platform event.
   */
  async publishSystemEvent(event: string, payload: Record<string, unknown>): Promise<void> {
    const channel = ChannelNamespace.system(event);
    await this.publish(channel, {
      version: '1',
      timestamp: new Date().toISOString(),
      source: 'sera-core',
      event,
      payload,
    });
  }

  // ── Circle Broadcast ───────────────────────────────────────────────────────

  /**
   * Send a broadcast message to a circle.
   */
  async broadcastToCircle(
    fromManifest: AgentManifest,
    circleId: string,
    payload: Record<string, unknown>
  ): Promise<IntercomMessage> {
    const fromAgentId = fromManifest.metadata.name;
    const agentCircle = fromManifest.metadata.circle;
    const additionalCircles = fromManifest.metadata.additionalCircles ?? [];

    if (agentCircle !== circleId && !additionalCircles.includes(circleId)) {
      throw new IntercomError(
        `Agent "${fromAgentId}" is not a member of circle "${circleId}"`,
        `circle:${circleId}`
      );
    }

    const channel = ChannelNamespace.circle(circleId);
    return this.publishMessage(fromAgentId, agentCircle ?? '', channel, 'message', payload, {
      securityTier: fromManifest.metadata.tier,
    });
  }

  // ── Config Retrieval ───────────────────────────────────────────────────────

  /**
   * Get the list of channels an agent is allowed to interact with,
   * based on its manifest configuration.
   */
  getAgentChannels(manifest: AgentManifest): {
    thoughts: string;
    status: string;
    tokens: string;
    dmPeers: string[];
    circles: string[];
  } {
    const agentId = manifest.metadata.name;
    const circles = [manifest.metadata.circle, ...(manifest.metadata.additionalCircles ?? [])];

    const dmPeers = (manifest.intercom?.canMessage ?? []).map((peerId) => {
      return ChannelNamespace.private(agentId, peerId);
    });

    return {
      thoughts: ChannelNamespace.thoughts(agentId),
      status: ChannelNamespace.status(agentId),
      tokens: ChannelNamespace.tokens(agentId),
      dmPeers,
      circles: circles
        .filter((c): c is string => c !== undefined)
        .map((c) => ChannelNamespace.circle(c)),
    };
  }

  // ── Tokens (Story 9.5) ─────────────────────────────────────────────────────

  /**
   * Generate a JWT for a user/agent to connect to Centrifugo.
   */
  async generateConnectionToken(userId: string): Promise<string> {
    const secret = new TextEncoder().encode(CENTRIFUGO_TOKEN_SECRET);
    return new jose.SignJWT({ sub: userId })
      .setProtectedHeader({ alg: 'HS256' })
      .setIssuedAt()
      .setExpirationTime('24h')
      .sign(secret);
  }

  /**
   * Generate a JWT for a user/agent to subscribe to a specific channel.
   * Story 9.5: role-based channel access matrix.
   */
  async generateSubscriptionToken(userId: string, channel: string, role: string): Promise<string> {
    const prefix = ChannelNamespace.getPrefix(channel);

    // Role-based access control
    if (role === 'viewer') {
      if (prefix !== 'thoughts') {
        throw new IntercomError(
          `Role "viewer" is only permitted to subscribe to thought streams.`,
          channel
        );
      }
    } else if (role !== 'admin' && role !== 'operator') {
      throw new IntercomError(`Unauthorized role: ${role}`, channel);
    }

    const secret = new TextEncoder().encode(CENTRIFUGO_TOKEN_SECRET);
    return new jose.SignJWT({
      sub: userId,
      channel,
      role, // Include the operator's role as a claim
    })
      .setProtectedHeader({ alg: 'HS256' })
      .setIssuedAt()
      .setExpirationTime('1h') // Expire in 1 hour
      .sign(secret);
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
      `private:${fromAgent}:${toAgent}`
    );
    this.name = 'IntercomPermissionError';
  }
}
