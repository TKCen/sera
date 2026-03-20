/**
 * IdentityService — JWT token issuer and verifier for SERA agents.
 *
 * Issues short-lived tokens to agent containers on spawn. These tokens
 * authenticate requests to internal services (LLM Proxy, Centrifugo Bus).
 *
 * Uses jose (HS256) for token signing and verification.
 *
 * @see docs/epics/04-llm-proxy-and-governance.md § Story 4.2
 */

import { SignJWT, jwtVerify } from 'jose';
import crypto from 'crypto';
import type { AgentTokenPayload, AgentTokenClaims } from './types.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('IdentityService');

const DEFAULT_EXPIRY = '1h';

export class IdentityService {
  private readonly secretBytes: Uint8Array;

  constructor(secret?: string) {
    let secretStr: string;
    if (secret) {
      secretStr = secret;
    } else if (process.env.JWT_SECRET) {
      secretStr = process.env.JWT_SECRET;
    } else {
      secretStr = crypto.randomBytes(32).toString('hex');
      logger.warn(
        'JWT_SECRET not set — using a random ephemeral secret. ' +
          'Tokens will not survive server restarts. Set JWT_SECRET in production.'
      );
    }
    this.secretBytes = new TextEncoder().encode(secretStr);
  }

  /**
   * Sign a short-lived JWT for an agent container.
   * Includes agentId, agentName, circleId, capabilities, and scope.
   */
  async signToken(
    payload: Omit<AgentTokenPayload, 'scope' | 'agentName'> &
      Partial<Pick<AgentTokenPayload, 'scope' | 'agentName'>>,
    expiresIn: string = DEFAULT_EXPIRY
  ): Promise<string> {
    const claims: AgentTokenPayload = {
      agentId: payload.agentId,
      agentName: payload.agentName ?? payload.agentId,
      circleId: payload.circleId,
      capabilities: payload.capabilities,
      scope: payload.scope ?? 'agent',
    };

    return new SignJWT({ ...claims })
      .setProtectedHeader({ alg: 'HS256' })
      .setIssuedAt()
      .setExpirationTime(expiresIn)
      .sign(this.secretBytes);
  }

  /**
   * Verify a JWT and return its decoded claims.
   * Throws if the token is expired, invalid, or tampered with.
   */
  async verifyToken(token: string): Promise<AgentTokenClaims> {
    const { payload } = await jwtVerify(token, this.secretBytes, {
      algorithms: ['HS256'],
    });

    const { agentId, agentName, circleId, capabilities, scope, iat, exp } = payload as Record<
      string,
      unknown
    >;

    if (typeof agentId !== 'string') throw new Error('Invalid token: missing agentId');
    if (typeof circleId !== 'string') throw new Error('Invalid token: missing circleId');

    return {
      agentId,
      agentName: typeof agentName === 'string' ? agentName : agentId,
      circleId,
      capabilities: Array.isArray(capabilities) ? (capabilities as string[]) : [],
      scope: (scope === 'internal' ? 'internal' : 'agent') as 'agent' | 'internal',
      iat: typeof iat === 'number' ? iat : 0,
      exp: typeof exp === 'number' ? exp : 0,
    };
  }
}
