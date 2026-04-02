import { describe, it, expect, vi, beforeEach } from 'vitest';

const mockQuery = vi.fn();

vi.mock('pg', () => {
  return {
    default: {
      Pool: class {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Mock argument matching signature
        query(text: string, params?: any[]) {
          return mockQuery(text, params);
        }
      },
    },
    Pool: class {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any -- Mock argument matching signature
      query(text: string, params?: any[]) {
        return mockQuery(text, params);
      }
    },
  };
});

const { mockMigrate } = vi.hoisted(() => ({
  mockMigrate: vi.fn(),
}));

vi.mock('node-pg-migrate', () => {
  return {
    default: mockMigrate,
  };
});

import { query, initDb } from './database.js';
import * as migrate from 'node-pg-migrate';
import path from 'path';

describe('database', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('query', () => {
    it('calls pool.query with correct arguments', async () => {
      const text = 'SELECT * FROM test';
      const params = [1, 2, 3];

      mockQuery.mockResolvedValueOnce({ rows: [] });

      await query(text, params);

      expect(mockQuery).toHaveBeenCalledWith(text, params);
    });

    it('handles query without params', async () => {
      const text = 'SELECT 1';

      mockQuery.mockResolvedValueOnce({ rows: [] });

      await query(text);

      expect(mockQuery).toHaveBeenCalledWith(text, undefined);
    });
  });

  describe('initDb', () => {
    it('runs migrations successfully', async () => {
      process.env.DATABASE_URL = 'postgres://test:test@localhost:5432/test';
      mockMigrate.mockResolvedValueOnce(undefined);

      await initDb();

      expect(mockMigrate).toHaveBeenCalledWith({
        databaseUrl: 'postgres://test:test@localhost:5432/test',
        dir: path.resolve(import.meta.dirname, '..', '..', 'src', 'db', 'migrations'),
        direction: 'up',
        migrationsTable: 'pgmigrations',
        verbose: true,
      });
    });

    it('throws error when migration fails', async () => {
      process.env.DATABASE_URL = 'postgres://test:test@localhost:5432/test';
      const error = new Error('Migration failed');
      mockMigrate.mockRejectedValueOnce(error);

      await expect(initDb()).rejects.toThrow('Migration failed');
    });
  });
});
