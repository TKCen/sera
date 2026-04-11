import { PgBoss } from 'pg-boss';
import { Logger } from './logger.js';

const logger = new Logger('PgBossService');

/**
 * Shared pg-boss instance used by all services (ScheduleService,
 * NotificationService, MemoryCompactionService).
 *
 * Keeps a single DB connection pool and a single polling loop instead of
 * three, which measurably reduces startup latency on an empty database.
 */
export class PgBossService {
  private static instance: PgBossService;
  private boss: PgBoss | null = null;

  private constructor() {}

  static getInstance(): PgBossService {
    if (!PgBossService.instance) {
      PgBossService.instance = new PgBossService();
    }
    return PgBossService.instance;
  }

  async start(databaseUrl: string): Promise<PgBoss> {
    if (this.boss) return this.boss;

    this.boss = new PgBoss(databaseUrl);
    this.boss.on('error', (err: unknown) => {
      logger.warn('pg-boss error:', err);
    });
    await this.boss.start();
    logger.info('pg-boss started');
    return this.boss;
  }

  getBoss(): PgBoss {
    if (!this.boss) throw new Error('PgBossService not started');
    return this.boss;
  }

  async stop(): Promise<void> {
    if (this.boss) {
      await this.boss.stop();
      this.boss = null;
      logger.info('pg-boss stopped');
    }
  }
}
