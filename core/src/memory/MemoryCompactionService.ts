/**
 * Epic 8 — MemoryCompactionService (Story 8.7)
 *
 * Runs as a pg-boss scheduled job (daily). Identifies personal memory blocks
 * that are older than MEMORY_ARCHIVE_AFTER_DAYS (default 30) with
 * importance <= 2, moves them to the archive directory, and removes them
 * from Qdrant.
 *
 * Blocks are never deleted — only moved to {memoryRoot}/{agentId}/archive/.
 */

import type { PgBoss } from 'pg-boss';
import { ScopedMemoryBlockStore } from './blocks/ScopedMemoryBlockStore.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('MemoryCompactionService');

const ARCHIVE_AFTER_DAYS = parseInt(process.env.MEMORY_ARCHIVE_AFTER_DAYS ?? '30', 10);

const MEMORY_ROOT = process.env.MEMORY_PATH ?? '/memory';
const JOB_NAME = 'memory-compaction';

export interface CompactionResult {
  agentId: string;
  blocksArchived: number;
}

export class MemoryCompactionService {
  private static instance: MemoryCompactionService;
  private boss: PgBoss | null = null;
  private store = new ScopedMemoryBlockStore(MEMORY_ROOT);

  private constructor() {}

  static getInstance(): MemoryCompactionService {
    if (!MemoryCompactionService.instance) {
      MemoryCompactionService.instance = new MemoryCompactionService();
    }
    return MemoryCompactionService.instance;
  }

  /** Returns true when MEMORY_COMPACTION_ENABLED=true is set in the environment. */
  static isEnabled(): boolean {
    return process.env.MEMORY_COMPACTION_ENABLED === 'true';
  }

  /** Register the daily compaction job on the shared pg-boss instance. */
  async start(boss: PgBoss): Promise<void> {
    this.boss = boss;

    // Queue must exist before scheduling
    await this.boss.createQueue(JOB_NAME);
    // Run daily at 03:00
    await this.boss.schedule(JOB_NAME, '0 3 * * *', {});
    await this.boss.work<{ agentId?: string }>(JOB_NAME, async (jobs) => {
      const job = Array.isArray(jobs) ? jobs[0] : jobs;
      const agentId = (job as { data?: { agentId?: string } }).data?.agentId;
      if (agentId) {
        await this.compactAgent(agentId);
      } else {
        await this.compactAll();
      }
    });
    logger.info(`MemoryCompactionService started — archiving after ${ARCHIVE_AFTER_DAYS} days`);
  }

  /** Manual trigger for a single agent. */
  async triggerCompaction(agentId: string): Promise<CompactionResult> {
    return this.compactAgent(agentId);
  }

  private async compactAll(): Promise<void> {
    // We can't easily enumerate all agent IDs without a separate registry,
    // so this path is a no-op in the scheduled case unless we have a DB query.
    // DECISION: Scheduled compaction requires manual trigger per agent or
    // integration with AgentRegistry. The manual POST endpoint is the primary
    // operator-facing path.
    logger.info('MemoryCompactionService: scheduled compaction run (no-op without agentId)');
  }

  private async compactAgent(agentId: string): Promise<CompactionResult> {
    const cutoffDate = new Date();
    cutoffDate.setDate(cutoffDate.getDate() - ARCHIVE_AFTER_DAYS);
    const cutoffIso = cutoffDate.toISOString();

    const blocks = await this.store.list(agentId, { before: cutoffIso });
    const toArchive = blocks.filter((b) => b.importance <= 2);

    let blocksArchived = 0;

    for (const block of toArchive) {
      const moved = await this.store.moveToArchive(agentId, block.id);
      if (moved) {
        blocksArchived++;
      }
    }

    logger.info(`Compaction for agent ${agentId}: archived=${blocksArchived}`);
    return { agentId, blocksArchived };
  }

  async stop(): Promise<void> {
    // PgBoss lifecycle is managed by PgBossService singleton
    this.boss = null;
  }
}
