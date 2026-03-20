import axios, { type AxiosInstance } from 'axios';
import https from 'https';
import fs from 'fs';
import { Logger } from '../lib/logger.js';
import type { IntercomMessage } from './types.js';
import type { CircleRegistry } from '../circles/CircleRegistry.js';
import type { IntercomService } from './IntercomService.js';
import type { CircleConnectionAuth } from '../circles/types.js';

const logger = new Logger('Bridge');

/**
 * BridgeService — handles cross-instance federation for SERA.
 * It synchronizes Centrifugo channels between different SERA instances
 * using mTLS or token-based authentication.
 */
export class BridgeService {
  private intercom?: IntercomService;
  private registry?: CircleRegistry;
  private remoteClients: Map<string, AxiosInstance> = new Map();

  constructor() {}

  /**
   * Initialize the bridge service with required dependencies.
   */
  init(intercom: IntercomService, registry: CircleRegistry): void {
    this.intercom = intercom;
    this.registry = registry;
  }

  /**
   * Connect to a remote SERA instance (Story 9.6 stub).
   */
  connect(remoteUrl: string, token: string): void {
    logger.info(`Federation: Connecting to ${remoteUrl} (stub)`);
  }

  /**
   * Disconnect from all remote instances (Story 9.6 stub).
   */
  disconnect(): void {
    logger.info('Federation: Disconnecting all bridges (stub)');
    this.remoteClients.clear();
  }

  /**
   * Route a message through the federation bridge (Story 9.6 stub).
   */
  route(message: IntercomMessage): void {
    logger.info(
      `Federation: Routing message via bridge (stub) — channel=${message.target.channel}`
    );
  }

  /**
   * Get or create an HTTP client for a remote circle connection.
   */
  private getClient(circleName: string): AxiosInstance | undefined {
    if (this.remoteClients.has(circleName)) {
      return this.remoteClients.get(circleName);
    }

    // Find a local circle that has a connection to the target remote circle
    const localCircles = this.registry?.listCircles() || [];
    let connectionFound: any = null;

    for (const lc of localCircles) {
      const conn = lc.connections?.find((c) => c.circle === circleName);
      if (conn && conn.auth !== 'internal' && typeof conn.auth === 'object') {
        connectionFound = conn;
        break;
      }
    }

    if (!connectionFound) return undefined;

    const auth = connectionFound.auth as CircleConnectionAuth;
    if (!auth.endpoint) {
      logger.warn(`Connection to ${circleName} has no endpoint configured`);
      return undefined;
    }

    const axiosConfig: any = {
      baseURL: auth.endpoint,
      timeout: 10000,
      headers: {
        'Content-Type': 'application/json',
      },
    };

    if (auth.type === 'mtls' && auth.certPath && auth.keyPath) {
      try {
        const httpsAgent = new https.Agent({
          cert: fs.readFileSync(auth.certPath),
          key: fs.readFileSync(auth.keyPath),
          ca: auth.caPath ? fs.readFileSync(auth.caPath) : undefined,
          // For dev/test environments with self-signed certs
          rejectUnauthorized: process.env.NODE_TLS_REJECT_UNAUTHORIZED !== '0',
        });
        axiosConfig.httpsAgent = httpsAgent;
        logger.info(`Configured mTLS for connection to ${circleName}`);
      } catch (err: any) {
        logger.error(`Failed to load certificates for ${circleName}: ${err.message}`);
        return undefined;
      }
    } else if (auth.type === 'token' && auth.token) {
      axiosConfig.headers['Authorization'] = `Bearer ${auth.token}`;
      logger.info(`Configured token auth for connection to ${circleName}`);
    }

    const client = axios.create(axiosConfig);
    this.remoteClients.set(circleName, client);
    return client;
  }

  /**
   * Hook for IntercomService to forward local publications to remote instances.
   */
  async handleLocalPublish(channel: string, message: IntercomMessage): Promise<void> {
    // Only bridge channels are candidates for federation
    if (!channel.startsWith('bridge:')) return;

    const parts = channel.split(':');
    let targetCircle: string | undefined;

    if (parts[1] === 'dm') {
      // bridge:dm:{circleA}:{circleB}:{agentA}:{agentB}
      const circles = [parts[2], parts[3]].filter((p): p is string => !!p);
      targetCircle = this.findRemoteCircle(circles);
    } else {
      // bridge:{circleA}:{circleB}:{name}
      const circles = [parts[1], parts[2]].filter((p): p is string => !!p);
      targetCircle = this.findRemoteCircle(circles);
    }

    if (targetCircle) {
      const client = this.getClient(targetCircle);
      if (client) {
        try {
          // Avoid infinite loops by tagging the message or checking source
          if ((message.source as any).bridged) return;

          const bridgedMessage = {
            ...message,
            source: { ...message.source, bridged: true },
          };

          await client.post('/api/intercom/bridge/receive', { channel, message: bridgedMessage });
          logger.info(`Forwarded message to ${targetCircle} on channel ${channel}`);
        } catch (err: any) {
          logger.error(`Failed to bridge to ${targetCircle}: ${err.message}`);
        }
      }
    }
  }

  /**
   * Find which of the listed circles is a known remote connection.
   */
  private findRemoteCircle(circles: string[]): string | undefined {
    if (!this.registry) return undefined;

    for (const cName of circles) {
      // If it's not a local circle, check if we have a connection to it
      if (!this.registry.getCircle(cName)) {
        const hasConnection = this.registry
          .listCircles()
          .some((lc) => lc.connections?.some((conn) => conn.circle === cName));
        if (hasConnection) return cName;
      }
    }
    return undefined;
  }

  /**
   * Entry point for incoming bridged messages from remote instances.
   */
  async receive(channel: string, message: IntercomMessage): Promise<void> {
    if (!this.intercom) {
      throw new Error('BridgeService not initialized');
    }

    logger.info(`Received inbound bridged message for channel ${channel}`);
    // Publish to local Centrifugo without re-bridging
    await this.intercom.publish(channel, message);
  }
}
