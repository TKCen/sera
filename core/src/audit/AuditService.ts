import crypto from 'crypto';
import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('AuditService');

/**
 * Deterministic JSON stringify to ensure consistent hashing even if object keys are reordered.
 */
function stableStringify(obj: any): string {
  if (obj === null || typeof obj !== 'object') {
    return JSON.stringify(obj);
  }

  if (Array.isArray(obj)) {
    return '[' + obj.map(stableStringify).join(',') + ']';
  }

  const keys = Object.keys(obj).sort();
  return '{' + keys.map(k => `${JSON.stringify(k)}:${stableStringify(obj[k])}`).join(',') + '}';
}

export interface AuditEntry {
  id: number;
  agent_id: string;
  action: string;
  details: any;
  timestamp: string;
  previous_hash: string | null;
  hash: string;
}

/**
 * AuditService — Merkle Hash-Chain Audit Trail for agent actions.
 *
 * Provides tamper-evidential tracking of tool calls and memory writes.
 * Each entry is linked to the previous one by including the previous
 * entry's hash in its own hash calculation.
 */
export class AuditService {
  private static instance: AuditService;

  private constructor() {}

  public static getInstance(): AuditService {
    if (!AuditService.instance) {
      AuditService.instance = new AuditService();
    }
    return AuditService.instance;
  }

  /**
   * Record a new action in the audit trail.
   */
  async record(agentId: string, action: string, details: any): Promise<void> {
    try {
      const timestamp = new Date().toISOString();

      // 1. Get the latest hash for this agent
      const latestResult = await query(
        'SELECT hash FROM audit_trail WHERE agent_id = $1 ORDER BY id DESC LIMIT 1',
        [agentId]
      );

      const previousHash = latestResult.rows.length > 0 ? latestResult.rows[0].hash : null;

      // 2. Compute current hash: SHA-256(previousHash + action + stableString(details) + timestamp)
      const dataToHash = `${previousHash || ''}${action}${stableStringify(details)}${timestamp}`;
      const hash = crypto.createHash('sha256').update(dataToHash).digest('hex');

      // 3. Store in DB
      await query(
        'INSERT INTO audit_trail (agent_id, action, details, timestamp, previous_hash, hash) VALUES ($1, $2, $3, $4, $5, $6)',
        [agentId, action, JSON.stringify(details), timestamp, previousHash, hash]
      );
    } catch (err) {
      logger.error(`Failed to record audit entry for agent ${agentId}:`, err);
    }
  }

  /**
   * Verify the integrity of an agent's audit trail.
   * Returns true if the chain is valid, false if tampered.
   */
  async verify(agentId: string): Promise<{ valid: boolean; brokenAt?: number; reason?: string }> {
    try {
      const result = await query(
        'SELECT * FROM audit_trail WHERE agent_id = $1 ORDER BY id ASC',
        [agentId]
      );

      const entries = result.rows as AuditEntry[];

      if (entries.length === 0) {
        return { valid: true };
      }

      let expectedPreviousHash: string | null = null;

      for (const entry of entries) {
        // 1. Check if previous_hash matches what we expected
        if (entry.previous_hash !== expectedPreviousHash) {
          return {
            valid: false,
            brokenAt: entry.id,
            reason: `Previous hash mismatch. Expected ${expectedPreviousHash}, got ${entry.previous_hash}`
          };
        }

        // 2. Recompute hash and verify
        const ts = typeof entry.timestamp === 'string' ? entry.timestamp : (entry.timestamp as any).toISOString();
        const dataToHash: string = `${entry.previous_hash || ''}${entry.action}${stableStringify(entry.details)}${ts}`;
        const computedHash: string = crypto.createHash('sha256').update(dataToHash).digest('hex');

        if (entry.hash !== computedHash) {
          return {
            valid: false,
            brokenAt: entry.id,
            reason: `Hash mismatch. Recomputed: ${computedHash}, stored: ${entry.hash}`
          };
        }

        expectedPreviousHash = entry.hash;
      }

      return { valid: true };
    } catch (err: any) {
      logger.error(`Audit verification failed for agent ${agentId}:`, err);
      return { valid: false, reason: err.message };
    }
  }

  /**
   * Get the audit trail for an agent.
   */
  async getTrail(agentId: string): Promise<AuditEntry[]> {
    const result = await query(
      'SELECT * FROM audit_trail WHERE agent_id = $1 ORDER BY timestamp ASC',
      [agentId]
    );
    return result.rows;
  }
}
