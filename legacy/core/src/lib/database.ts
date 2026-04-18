import pg from 'pg';
import { Logger } from './logger.js';
import * as migrateModule from 'node-pg-migrate';
const migrate = (migrateModule as any).runner ?? (migrateModule as any).default ?? migrateModule;
import path from 'path';

const logger = new Logger('Database');

const { Pool } = pg;

export const pool = new Pool({
  connectionString: process.env.DATABASE_URL,
});

export const query = (text: string, params?: unknown[]) => pool.query(text, params);

export const initDb = async () => {
  try {
    const migrationsDir = path.resolve(import.meta.dirname, '..', '..', 'src', 'db', 'migrations');

    const runner =
      (migrate as unknown as { default: (o: Record<string, unknown>) => Promise<void> }).default ||
      migrate;
    await runner({
      databaseUrl: process.env.DATABASE_URL!,
      dir: migrationsDir,
      direction: 'up',
      migrationsTable: 'pgmigrations',
      verbose: true,
    });

    logger.info('Database migrations completed successfully');
  } catch (err) {
    logger.error('Database migration failed:', err);
    throw err;
  }
};
