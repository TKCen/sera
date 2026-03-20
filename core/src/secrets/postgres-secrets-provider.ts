import crypto from 'crypto';
import { query } from '../lib/database.js';
import type {
  SecretsProvider,
  SecretAccessContext,
  SecretMetadata,
  SecretFilter,
} from './interfaces.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('PostgresSecretsProvider');
const ALGORITHM = 'aes-256-gcm';
const IV_LENGTH = 12; // 96 bits for GCM
const AUTH_TAG_LENGTH = 16;

export class PostgresSecretsProvider implements SecretsProvider {
  readonly id = 'postgres';
  private readonly masterKey: Buffer;

  constructor() {
    const keyStr = process.env.SECRETS_MASTER_KEY;
    if (!keyStr) {
      throw new Error('SECRETS_MASTER_KEY not set');
    }

    this.masterKey = Buffer.from(keyStr, 'hex');
    if (this.masterKey.length !== 32) {
      throw new Error('SECRETS_MASTER_KEY must be a 32-byte hex string (64 characters)');
    }
  }

  private encrypt(value: string): { encryptedValue: Buffer; iv: Buffer } {
    const iv = crypto.randomBytes(IV_LENGTH);
    const cipher = crypto.createCipheriv(ALGORITHM, this.masterKey, iv, {
      authTagLength: AUTH_TAG_LENGTH,
    });

    const encryptedContent = Buffer.concat([cipher.update(value, 'utf8'), cipher.final()]);

    const authTag = cipher.getAuthTag();
    const encryptedValue = Buffer.concat([encryptedContent, authTag]);

    return { encryptedValue, iv };
  }

  private decrypt(encryptedValue: Buffer, iv: Buffer): string {
    const authTag = encryptedValue.subarray(encryptedValue.length - AUTH_TAG_LENGTH);
    const encryptedContent = encryptedValue.subarray(0, encryptedValue.length - AUTH_TAG_LENGTH);

    const decipher = crypto.createDecipheriv(ALGORITHM, this.masterKey, iv, {
      authTagLength: AUTH_TAG_LENGTH,
    });
    decipher.setAuthTag(authTag);

    const decrypted = Buffer.concat([decipher.update(encryptedContent), decipher.final()]);

    return decrypted.toString('utf8');
  }

  async get(name: string, context: SecretAccessContext): Promise<string | null> {
    const result = await query(
      'SELECT encrypted_value, iv, allowed_agents FROM secrets WHERE name = $1 AND deleted_at IS NULL',
      [name]
    );

    if (result.rowCount === 0) {
      return null;
    }

    const { encrypted_value, iv, allowed_agents } = result.rows[0];

    // Access control: operator has full access, agent must be in allowed_agents
    if (!context.operator) {
      if (!allowed_agents.includes(context.agentName) && !allowed_agents.includes('*')) {
        logger.warn(`Access denied to secret "${name}" for agent "${context.agentName}"`);
        return null;
      }
    }

    try {
      return this.decrypt(encrypted_value, iv);
    } catch (err) {
      logger.error(`Failed to decrypt secret "${name}":`, err);
      throw new Error('Secret decryption failed');
    }
  }

  async set(name: string, value: string, metadata?: Partial<SecretMetadata>): Promise<void> {
    const { encryptedValue, iv } = this.encrypt(value);

    await query(
      `INSERT INTO secrets (
        name, encrypted_value, iv, description, allowed_agents, tags, exposure, updated_at
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
      ON CONFLICT (name) DO UPDATE SET
        encrypted_value = EXCLUDED.encrypted_value,
        iv = EXCLUDED.iv,
        description = COALESCE(EXCLUDED.description, secrets.description),
        allowed_agents = COALESCE(EXCLUDED.allowed_agents, secrets.allowed_agents),
        tags = COALESCE(EXCLUDED.tags, secrets.tags),
        exposure = COALESCE(EXCLUDED.exposure, secrets.exposure),
        updated_at = NOW()`,
      [
        name,
        encryptedValue,
        iv,
        metadata?.description ?? null,
        metadata?.allowedAgents ?? [],
        metadata?.tags ?? [],
        metadata?.exposure ?? 'per-call',
      ]
    );
  }

  async delete(name: string, context: SecretAccessContext): Promise<boolean> {
    // Only operators can delete for now
    if (!context.operator) {
      throw new Error('Unauthorized: only operators can delete secrets');
    }

    const result = await query(
      'UPDATE secrets SET deleted_at = NOW() WHERE name = $1 AND deleted_at IS NULL',
      [name]
    );
    return (result.rowCount ?? 0) > 0;
  }

  async list(filter: SecretFilter, context: SecretAccessContext): Promise<SecretMetadata[]> {
    // Only operators can list for now
    if (!context.operator) {
      throw new Error('Unauthorized: only operators can list secrets');
    }

    let sql = 'SELECT * FROM secrets WHERE deleted_at IS NULL';
    const params: unknown[] = [];

    if (filter?.tags && filter.tags.length > 0) {
      sql += ' AND tags && $1';
      params.push(filter.tags);
    }

    if (filter?.agentId) {
      // In a real scenario, we might filter by what an agent can see,
      // but for operator list it shows everything.
    }

    const result = await query(sql, params);
    return result.rows.map((row) => ({
      id: row.id,
      name: row.name,
      description: row.description,
      allowedAgents: row.allowed_agents,
      allowedCircles: row.allowed_circles ?? [],
      tags: row.tags,
      exposure: row.exposure,
      createdAt: row.created_at,
      updatedAt: row.updated_at,
      rotatedAt: row.rotated_at,
      expiresAt: row.expires_at,
    }));
  }

  async healthCheck(): Promise<boolean> {
    try {
      await query('SELECT 1');
      return true;
    } catch {
      return false;
    }
  }
}
