import { createRemoteJWKSet, jwtVerify } from 'jose';
import type { Request } from 'express';
import type { AuthPlugin, OperatorIdentity, OperatorRole } from './interfaces.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('OIDCAuthProvider');

export class OIDCAuthPlugin implements AuthPlugin {
  readonly name = 'oidc';

  private readonly issuerUrl: string;
  private readonly clientId: string;
  private readonly clientSecret: string | undefined;
  private readonly audience: string | undefined;
  private readonly groupsClaim: string;
  private readonly roleMapping: Record<string, OperatorRole>;
  private readonly cacheTtlMs: number;

  private jwksSet: ReturnType<typeof createRemoteJWKSet> | null = null;

  constructor() {
    const issuerUrl = process.env.OIDC_ISSUER_URL;
    if (!issuerUrl) {
      throw new Error('OIDC_ISSUER_URL is not set');
    }
    this.issuerUrl = issuerUrl.replace(/\/$/, '');
    this.clientId = process.env.OIDC_CLIENT_ID ?? 'sera-web';
    this.clientSecret = process.env.OIDC_CLIENT_SECRET;
    this.audience = process.env.OIDC_AUDIENCE;
    this.groupsClaim = process.env.OIDC_GROUPS_CLAIM ?? 'groups';

    const ttlSeconds = parseInt(process.env.OIDC_JWKS_CACHE_TTL_SECONDS ?? '3600', 10);
    this.cacheTtlMs = ttlSeconds * 1000;

    try {
      this.roleMapping = process.env.OIDC_ROLE_MAPPING
        ? (JSON.parse(process.env.OIDC_ROLE_MAPPING) as Record<string, OperatorRole>)
        : {};
    } catch {
      logger.warn('OIDC_ROLE_MAPPING is not valid JSON, using empty mapping');
      this.roleMapping = {};
    }

    // Initialise JWKS set on startup (lazy — first actual verification triggers the HTTP fetch)
    this.getJWKS();
  }

  /**
   * Returns a cached createRemoteJWKSet handle. jose's implementation internally
   * refreshes on kid-mismatch and respects cacheMaxAge.
   */
  private getJWKS(): ReturnType<typeof createRemoteJWKSet> {
    if (!this.jwksSet) {
      const jwksUrl = new URL(`${this.issuerUrl}/.well-known/jwks.json`);
      this.jwksSet = createRemoteJWKSet(jwksUrl, {
        cacheMaxAge: this.cacheTtlMs,
      });
    }
    return this.jwksSet;
  }

  async authenticate(req: Request): Promise<OperatorIdentity | null> {
    const authHeader = req.headers.authorization;
    if (!authHeader?.startsWith('Bearer ')) return null;

    const token = authHeader.slice(7);

    // Skip tokens that are clearly not JWTs
    if (token.startsWith('sera_') || token.startsWith('sess_')) return null;
    const parts = token.split('.');
    if (parts.length !== 3) return null;

    try {
      const verifyOptions: Parameters<typeof jwtVerify>[2] = {
        issuer: this.issuerUrl,
        algorithms: [
          'RS256', 'RS384', 'RS512',
          'PS256', 'PS384', 'PS512',
          'ES256', 'ES384', 'ES512',
        ],
      };
      if (this.audience) {
        verifyOptions.audience = this.audience;
      }

      const jwks = this.getJWKS();
      const { payload } = await jwtVerify(token, jwks, verifyOptions);

      const rawGroups = payload[this.groupsClaim];
      const groups = Array.isArray(rawGroups) ? (rawGroups as string[]) : [];
      const roles = this.mapGroupsToRoles(groups);

      const identity: OperatorIdentity = {
        sub: payload.sub!,
        roles,
        authMethod: 'oidc',
      };
      if (typeof payload['email'] === 'string') identity.email = payload['email'];
      if (typeof payload['name'] === 'string') identity.name = payload['name'];
      if (!identity.name && typeof payload['preferred_username'] === 'string') {
        identity.name = payload['preferred_username'];
      }

      return identity;
    } catch (err: any) {
      // Token looks like a JWT but validation failed — throw so caller gets 401
      if (err?.code === 'ERR_JWT_EXPIRED') {
        const e = new Error('invalid_token') as any;
        e.statusCode = 401;
        e.hint = 'The access token has expired. Please re-authenticate.';
        throw e;
      }
      if (
        err?.code?.startsWith('ERR_JWT') ||
        err?.code?.startsWith('ERR_JWS') ||
        err?.code?.startsWith('ERR_JWK')
      ) {
        const e = new Error('invalid_token') as any;
        e.statusCode = 401;
        e.hint = err.message ?? 'Token validation failed';
        throw e;
      }
      // Not recognisably ours — let another plugin try
      return null;
    }
  }

  private mapGroupsToRoles(groups: string[]): OperatorRole[] {
    const roles = new Set<OperatorRole>();
    for (const group of groups) {
      const mapped = this.roleMapping[group];
      if (mapped) roles.add(mapped);
    }
    if (roles.size === 0) roles.add('viewer');
    return Array.from(roles);
  }
}
