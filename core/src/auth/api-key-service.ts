import crypto from 'node:crypto';
import argon2 from 'argon2';
import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import type { OperatorRole } from './interfaces.js';

const logger = new Logger('ApiKeyService');

export interface ApiKeyMetadata {
  id: string;
  name: string;
  ownerSub: string;
  roles: OperatorRole[];
  createdAt: Date;
  lastUsedAt: Date | null;
  expiresAt: Date | null;
}

export class ApiKeyService {
  /**
   * Create a new API key.
   * Returns the plain-text key (to be shown once) and the metadata.
   */
  static async createKey(params: {
    name: string;
    ownerSub: string;
    roles: OperatorRole[];
    expiresInDays?: number;
  }): Promise<{ key: string; metadata: ApiKeyMetadata }> {
    // Generate 32 bytes of random data -> 64 hex chars
    const randomPart = crypto.randomBytes(32).toString('hex');
    const key = `sera_${randomPart}`;

    const keyHash = await argon2.hash(key);
    
    let expiresAt: Date | null = null;
    if (params.expiresInDays) {
      expiresAt = new Date();
      expiresAt.setDate(expiresAt.getDate() + params.expiresInDays);
    }

    const result = await query(
      `INSERT INTO api_keys (name, key_hash, owner_sub, roles, expires_at)
       VALUES ($1, $2, $3, $4, $5)
       RETURNING id, name, owner_sub as "ownerSub", roles, created_at as "createdAt", last_used_at as "lastUsedAt", expires_at as "expiresAt"`,
      [params.name, keyHash, params.ownerSub, params.roles, expiresAt]
    );

    const metadata = result.rows[0] as ApiKeyMetadata;
    logger.info(`Created API key "${params.name}" for ${params.ownerSub}`);

    return { key, metadata };
  }

  /**
   * List API keys for an owner.
   */
  static async listKeys(ownerSub: string): Promise<ApiKeyMetadata[]> {
    const result = await query(
      `SELECT id, name, owner_sub as "ownerSub", roles, created_at as "createdAt", last_used_at as "lastUsedAt", expires_at as "expiresAt"
       FROM api_keys
       WHERE owner_sub = $1 AND revoked_at IS NULL
       ORDER BY created_at DESC`,
      [ownerSub]
    );
    return result.rows as ApiKeyMetadata[];
  }

  /**
   * Revoke an API key.
   */
  static async revokeKey(id: string, ownerSub: string): Promise<boolean> {
    const result = await query(
      `UPDATE api_keys 
       SET revoked_at = NOW() 
       WHERE id = $1 AND owner_sub = $2 AND revoked_at IS NULL`,
      [id, ownerSub]
    );
    return (result.rowCount ?? 0) > 0;
  }
}
