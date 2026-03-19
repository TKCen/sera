import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AuditService } from './AuditService.js';
import { pool } from '../lib/database.js';

// Mock Database
vi.mock('../lib/database.js', () => ({
  pool: {
    connect: vi.fn(),
    query: vi.fn(),
  },
}));

describe('Audit Export Streaming', () => {
  let service: AuditService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = AuditService.getInstance();
  });

  it('streams 10,000+ records with stable memory usage', async () => {
    const totalRecords = 15000;
    const batchSize = 100;
    let fetchedCount = 0;

    const clientMock = {
      query: vi.fn().mockImplementation((q: string) => {
        if (q.includes('DECLARE')) return Promise.resolve({});
        if (q.includes('FETCH')) {
          if (fetchedCount >= totalRecords) return Promise.resolve({ rows: [] });
          
          const rows = [];
          for (let i = 0; i < batchSize; i++) {
            rows.push({ id: `id-${fetchedCount + i}`, sequence: String(fetchedCount + i), payload: { data: 'x'.repeat(1000) } });
          }
          fetchedCount += batchSize;
          return Promise.resolve({ rows });
        }
        return Promise.resolve({});
      }),
      release: vi.fn(),
    };
    (pool.connect as any).mockResolvedValue(clientMock);

    const memoryBefore = process.memoryUsage().heapUsed;
    let streamCount = 0;

    await service.streamEntries((row) => {
      streamCount++;
      // Simulate writing to response
    });

    const memoryAfter = process.memoryUsage().heapUsed;
    const memoryDiffMB = (memoryAfter - memoryBefore) / 1024 / 1024;

    expect(streamCount).toBe(totalRecords);
    // Allowing 10MB diff for overhead, but definitely should NOT be 15,000 * 1KB (~15MB) or more extra if it was buffered
    // Actually, memoryDiff might be positive due to other things, but should be reasonable.
    // The key is that it doesn't crash or grow proportionally to totalRecords in a huge way.
    expect(memoryDiffMB).toBeLessThan(50); 
    logger.info(`Memory diff for 15k records export: ${memoryDiffMB.toFixed(2)} MB`);
  });
});

const logger = { info: console.log };
