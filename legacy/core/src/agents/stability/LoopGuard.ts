/**
 * LoopGuard — detects degenerate tool-calling patterns in the agent loop.
 *
 * Hashes each (toolName, params) tuple with a simple string hash and tracks
 * repeat counts. Returns a status indicating whether the call should proceed.
 *
 * Thresholds:
 *  - ok   : count < WARN_THRESHOLD  (proceed normally)
 *  - warn : count >= WARN_THRESHOLD (proceed with advisory)
 *  - block: count >= BLOCK_THRESHOLD (skip execution)
 */

import { Logger } from '../../lib/logger.js';

const logger = new Logger('LoopGuard');

/** Number of identical calls before issuing a warning. */
const WARN_THRESHOLD = 3;

/** Number of identical calls before blocking execution. */
const BLOCK_THRESHOLD = 5;

export type LoopGuardStatus = 'ok' | 'warn' | 'block';

export interface LoopGuardResult {
  status: LoopGuardStatus;
  count: number;
  hash: string;
}

export class LoopGuard {
  private callCounts = new Map<string, number>();

  /**
   * Record a tool call and return whether it should proceed.
   */
  recordCall(toolName: string, params: unknown): LoopGuardResult {
    const hash = LoopGuard.hashCall(toolName, params);
    const prev = this.callCounts.get(hash) ?? 0;
    const count = prev + 1;
    this.callCounts.set(hash, count);

    let status: LoopGuardStatus = 'ok';

    if (count >= BLOCK_THRESHOLD) {
      status = 'block';
      logger.warn(
        `BLOCKED duplicate tool call: "${toolName}" called ${count} times with same args`
      );
    } else if (count >= WARN_THRESHOLD) {
      status = 'warn';
      logger.warn(
        `WARNING: "${toolName}" called ${count} times with same args (block at ${BLOCK_THRESHOLD})`
      );
    }

    return { status, count, hash };
  }

  /** Reset all tracked state (call between sessions). */
  reset(): void {
    this.callCounts.clear();
  }

  /** Get total unique call hashes tracked. */
  get size(): number {
    return this.callCounts.size;
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────

  /**
   * Create a deterministic string hash for a (toolName, params) pair.
   * Uses a fast DJB2-style hash for performance (cryptographic strength not needed).
   */
  static hashCall(toolName: string, params: unknown): string {
    const input = `${toolName}::${JSON.stringify(params ?? {})}`;
    let hash = 5381;
    for (let i = 0; i < input.length; i++) {
      hash = ((hash << 5) + hash + input.charCodeAt(i)) | 0;
    }
    // Convert to unsigned 32-bit hex
    return (hash >>> 0).toString(16).padStart(8, '0');
  }
}
