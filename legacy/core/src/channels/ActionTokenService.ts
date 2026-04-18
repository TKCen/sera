import { SignJWT, jwtVerify } from 'jose';
import crypto from 'crypto';
import type { RequestType } from './channel.interface.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('ActionTokenService');

export interface ActionTokenClaims {
  sub: string; // requestId
  action: 'approve' | 'deny';
  requestType: RequestType;
  iss: 'sera';
  exp: number;
}

export interface ActionTokenPair {
  approveToken: string;
  denyToken: string;
  expiresAt: string;
}

let ephemeralSecret: Uint8Array | undefined;

function getSignKey(): Uint8Array {
  if (process.env['JWT_SECRET']) {
    return new TextEncoder().encode(process.env['JWT_SECRET']);
  }
  if (!ephemeralSecret) {
    ephemeralSecret = crypto.randomBytes(32);
    logger.warn(
      'JWT_SECRET not set for action tokens — using a random ephemeral secret. ' +
        'Action tokens will not survive server restarts. Set JWT_SECRET in production.'
    );
  }
  return ephemeralSecret;
}

const TOKEN_TTL_SECONDS = 15 * 60; // 15 minutes

export class ActionTokenService {
  private static instance: ActionTokenService;

  private constructor() {}

  static getInstance(): ActionTokenService {
    if (!ActionTokenService.instance) {
      ActionTokenService.instance = new ActionTokenService();
    }
    return ActionTokenService.instance;
  }

  async issue(requestId: string, requestType: RequestType): Promise<ActionTokenPair> {
    const key = getSignKey();
    const exp = Math.floor(Date.now() / 1000) + TOKEN_TTL_SECONDS;
    const expiresAt = new Date(exp * 1000).toISOString();

    const approveToken = await new SignJWT({
      sub: requestId,
      action: 'approve' as const,
      requestType,
    })
      .setProtectedHeader({ alg: 'HS256' })
      .setIssuer('sera')
      .setExpirationTime(exp)
      .sign(key);

    const denyToken = await new SignJWT({
      sub: requestId,
      action: 'deny' as const,
      requestType,
    })
      .setProtectedHeader({ alg: 'HS256' })
      .setIssuer('sera')
      .setExpirationTime(exp)
      .sign(key);

    return { approveToken, denyToken, expiresAt };
  }

  async verify(token: string): Promise<ActionTokenClaims> {
    const key = getSignKey();
    const { payload } = await jwtVerify(token, key, { issuer: 'sera' });

    if (
      typeof payload['sub'] !== 'string' ||
      (payload['action'] !== 'approve' && payload['action'] !== 'deny') ||
      typeof payload['requestType'] !== 'string'
    ) {
      throw new Error('Invalid action token claims');
    }

    return {
      sub: payload['sub'],
      action: payload['action'],
      requestType: payload['requestType'] as RequestType,
      iss: 'sera',
      exp: payload['exp'] as number,
    };
  }

  buildActionUrls(
    approveToken: string,
    denyToken: string
  ): { approveUrl: string; denyUrl: string } {
    const base = (process.env['SERA_PUBLIC_URL'] ?? 'http://localhost:3001').replace(/\/$/, '');
    return {
      approveUrl: `${base}/api/notifications/action?token=${encodeURIComponent(approveToken)}`,
      denyUrl: `${base}/api/notifications/action?token=${encodeURIComponent(denyToken)}`,
    };
  }
}
