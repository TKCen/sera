import crypto from 'crypto';
import type { OperatorIdentity } from './interfaces.js';

export interface WebSession {
  readonly id: string;
  readonly identity: OperatorIdentity;
  /** OIDC access token — never leaves the server */
  readonly accessToken?: string;
  /** OIDC refresh token — never leaves the server */
  readonly refreshToken?: string;
  readonly accessTokenExpiry?: Date;
  readonly createdAt: Date;
  readonly expiresAt: Date;
}

export class WebSessionStore {
  private readonly sessions = new Map<string, WebSession>();
  private readonly maxAgeMs: number;

  constructor() {
    const maxAgeSeconds = parseInt(process.env.SESSION_MAX_AGE_SECONDS ?? '28800', 10);
    this.maxAgeMs = maxAgeSeconds * 1000;

    // Purge expired sessions hourly
    setInterval(() => this.evictExpired(), 60 * 60 * 1_000).unref();
  }

  create(
    identity: OperatorIdentity,
    tokens?: {
      accessToken?: string;
      refreshToken?: string;
      accessTokenExpiry?: Date;
    }
  ): string {
    const id = `sess_${crypto.randomBytes(32).toString('hex')}`;
    const now = new Date();
    const session: WebSession = {
      id,
      identity,
      ...(tokens?.accessToken !== undefined ? { accessToken: tokens.accessToken } : {}),
      ...(tokens?.refreshToken !== undefined ? { refreshToken: tokens.refreshToken } : {}),
      ...(tokens?.accessTokenExpiry !== undefined
        ? { accessTokenExpiry: tokens.accessTokenExpiry }
        : {}),
      createdAt: now,
      expiresAt: new Date(now.getTime() + this.maxAgeMs),
    };
    this.sessions.set(id, session);
    return id;
  }

  get(id: string): WebSession | null {
    const session = this.sessions.get(id);
    if (!session) return null;
    if (session.expiresAt < new Date()) {
      this.sessions.delete(id);
      return null;
    }
    return session;
  }

  delete(id: string): void {
    this.sessions.delete(id);
  }

  private evictExpired(): void {
    const now = new Date();
    for (const [id, s] of this.sessions) {
      if (s.expiresAt < now) this.sessions.delete(id);
    }
  }
}
