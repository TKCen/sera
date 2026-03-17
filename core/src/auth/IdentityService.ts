/**
 * IdentityService — JWT token issuer and verifier for SERA agents.
 *
 * Issues short-lived tokens to agent containers on spawn. These tokens
 * authenticate requests to internal services (LLM Proxy, Centrifugo Bus).
 *
 * @see docs/v2-distributed-architecture/02-security-and-gateway.md § Identity & JWT Auth
 */

import jwt from 'jsonwebtoken';
import crypto from 'crypto';
import type { AgentTokenPayload, AgentTokenClaims } from './types.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('IdentityService');

const DEFAULT_EXPIRY = '1h';

export class IdentityService {
  private readonly secret: string;

  constructor(secret?: string) {
    if (secret) {
      this.secret = secret;
    } else if (process.env.JWT_SECRET) {
      this.secret = process.env.JWT_SECRET;
    } else {
      this.secret = crypto.randomBytes(32).toString('hex');
      logger.warn(
        'JWT_SECRET not set — using a random ephemeral secret. ' +
        'Tokens will not survive server restarts. Set JWT_SECRET in production.',
      );
    }
  }

  /**
   * Sign a short-lived JWT for an agent container.
   */
  signToken(payload: AgentTokenPayload, expiresIn: string = DEFAULT_EXPIRY): string {
    return jwt.sign(
      {
        agentId: payload.agentId,
        circleId: payload.circleId,
        capabilities: payload.capabilities,
      },
      this.secret,
      { expiresIn: expiresIn as unknown as number },
    );
  }

  /**
   * Verify a JWT and return its decoded claims.
   * Throws if the token is expired, invalid, or tampered with.
   */
  verifyToken(token: string): AgentTokenClaims {
    const decoded = jwt.verify(token, this.secret);

    // jwt.verify returns string | JwtPayload — we need a structured payload
    if (typeof decoded === 'string') {
      throw new Error('Invalid token format');
    }

    return decoded as AgentTokenClaims;
  }
}
