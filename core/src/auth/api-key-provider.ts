import type { Request } from 'express';
import argon2 from 'argon2';
import type { AuthPlugin, OperatorIdentity, OperatorRole } from './interfaces.js';
import { Logger } from '../lib/logger.js';
import { query } from '../lib/database.js';

const logger = new Logger('ApiKeyProvider');

export class ApiKeyProvider implements AuthPlugin {
  readonly name = 'api-key';
  private readonly bootstrapKey: string | undefined;

  constructor() {
    this.bootstrapKey = process.env.SERA_BOOTSTRAP_API_KEY;
    if (this.bootstrapKey) {
      logger.info('Bootstrap API key enabled');
    }
  }

  async authenticate(req: Request): Promise<OperatorIdentity | null> {
    const authHeader = req.headers.authorization;
    if (!authHeader || !authHeader.startsWith('Bearer ')) {
      return null;
    }

    const key = authHeader.slice(7);

    // Bootstrap key logic (only works if env var is set)
    // # DECISION: Bootstrap key gives 'admin' role and 'system' sub.
    if (this.bootstrapKey && key === this.bootstrapKey) {
      return {
        sub: 'system:bootstrap',
        name: 'Bootstrap Admin',
        roles: ['admin'],
        authMethod: 'api-key',
      };
    }

    // Standard API key logic
    if (!key.startsWith('sera_')) {
      return null;
    }

    const result = await query(
      'SELECT id, key_hash, owner_sub, roles FROM api_keys WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > NOW())'
    );

    let validRow: (typeof result.rows)[0] | null = null;
    for (const row of result.rows) {
      try {
        if (row.key_hash && (await argon2.verify(row.key_hash, key))) {
          validRow = row;
          break;
        }
      } catch (error) {
        // Ignore errors from invalid hashes to prevent crashing the entire check
        logger.warn(`Failed to verify API key hash for key ID ${row.id}: ${error}`);
      }
    }

    if (validRow) {
      // Update last_used_at async
      query('UPDATE api_keys SET last_used_at = NOW() WHERE id = $1', [validRow.id]).catch(
        () => {}
      );

      return {
        sub: validRow.owner_sub,
        roles: validRow.roles as OperatorRole[],
        authMethod: 'api-key',
      };
    }

    throw new Error('Invalid API key');
  }
}
