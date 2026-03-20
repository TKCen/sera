import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AuditService } from './AuditService.js';
import { pool } from '../lib/database.js';
import crypto from 'node:crypto';
import type { PoolClient } from 'pg';

// Mock Database
vi.mock('../lib/database.js', () => ({
  pool: {
    query: vi.fn(),
    connect: vi.fn().mockResolvedValue({
      query: vi.fn(),
      release: vi.fn(),
    }),
  },
}));

describe('AuditService', () => {
  let service: AuditService;

  beforeEach(() => {
    vi.clearAllMocks();
    service = AuditService.getInstance();
    (service as unknown as { initialized: boolean }).initialized = false;
    (service as unknown as { lastHash: string | null }).lastHash = null;
  });

  describe('record and Merkle chain', () => {
    it('creates a genesis record if empty', async () => {
      // Mock empty DB
      (pool.connect as import('vitest').Mock).mockResolvedValueOnce({
        query: vi
          .fn()
          .mockResolvedValueOnce({ rows: [] }) // Check if any records exist
          .mockResolvedValueOnce({ rows: [{ seq: '1' }] }) // nextval for genesis
          .mockResolvedValueOnce({ rows: [] }), // INSERT
        release: vi.fn(),
      });

      // Verification mock (called inside initialize)
      (pool.query as import('vitest').Mock).mockResolvedValueOnce({ rows: [] });

      await service.initialize();

      expect(pool.connect).toHaveBeenCalled();
    });

    it('computes hashes correctly linking to previous records', async () => {
      const clientMock = {
        query: vi.fn(),
        release: vi.fn(),
      };
      (pool.connect as import('vitest').Mock).mockResolvedValue(clientMock);

      // Setup for record()
      clientMock.query
        .mockResolvedValueOnce({ rows: [] }) // BEGIN
        .mockResolvedValueOnce({ rows: [] }) // LOCK
        .mockResolvedValueOnce({ rows: [{ hash: 'prev-hash' }] }) // Get last record hash
        .mockResolvedValueOnce({ rows: [{ seq: '10' }] }) // nextval
        .mockResolvedValueOnce({ rows: [] }) // INSERT
        .mockResolvedValueOnce({ rows: [] }); // COMMIT

      await service.record({
        actorType: 'agent',
        actorId: 'agent-1',
        actingContext: null,
        eventType: 'test.event',
        payload: { foo: 'bar' },
      });

      // Verify the insert call had a hash and prev_hash
      const insertCall = clientMock.query.mock.calls.find((c) =>
        (c[0] as string).includes('INSERT INTO audit_trail')
      );
      expect(insertCall).toBeDefined();
      if (insertCall) {
        const params = insertCall[1] as unknown[];
        expect(params[7]).toBe('prev-hash'); // prev_hash
        expect(params[8]).toBeDefined(); // hash
      }
    });
  });

  describe('verifyIntegrity', () => {
    it('returns valid: true for consistent chain', async () => {
      const timestamp = new Date();
      // Manual hash computation to match service
      const computeHash = (seq: string, prev: string | null) => {
        const canonical = [
          seq,
          timestamp.toISOString(),
          'agent',
          'agent-1',
          'test.event',
          JSON.stringify({ foo: 'bar' }),
          prev || '',
        ].join('|');
        return crypto.createHash('sha256').update(canonical).digest('hex');
      };

      const hash1 = computeHash('1', null);
      const hash2 = computeHash('2', hash1);

      (pool.query as import('vitest').Mock).mockResolvedValueOnce({
        rows: [
          {
            sequence: '2',
            timestamp,
            actor_type: 'agent',
            actor_id: 'agent-1',
            event_type: 'test.event',
            payload: { foo: 'bar' },
            prev_hash: hash1,
            hash: hash2,
          },
          {
            sequence: '1',
            timestamp,
            actor_type: 'agent',
            actor_id: 'agent-1',
            event_type: 'test.event',
            payload: { foo: 'bar' },
            prev_hash: null,
            hash: hash1,
          },
        ],
      });

      const result = await service.verifyIntegrity();
      expect(result.valid).toBe(true);
    });

    it('detects tampering when a record hash is invalid', async () => {
      const timestamp = new Date();
      (pool.query as import('vitest').Mock).mockResolvedValueOnce({
        rows: [
          {
            sequence: '1',
            timestamp,
            actor_type: 'agent',
            actor_id: 'agent-1',
            event_type: 'test.event',
            payload: { foo: 'bar' },
            prev_hash: null,
            hash: 'WRONG-HASH',
          },
        ],
      });

      const result = await service.verifyIntegrity();
      expect(result.valid).toBe(false);
      expect(result.brokenAt).toBe('1');
    });

    it('detects tampering when the chain link is broken', async () => {
      const timestamp = new Date();

      // Mock computeHash to return consistent hashes but we'll break the prev_hash link
      const serviceObj = service as unknown as {
        computeHash: (
          seq: string,
          timestamp: Date,
          actorType: string,
          actorId: string,
          eventType: string,
          payload: unknown,
          prevHash: string | null
        ) => string;
      };
      const originalComputeHash = serviceObj.computeHash;
      serviceObj.computeHash = vi.fn().mockReturnValueOnce('hash-1').mockReturnValueOnce('hash-2');

      (pool.query as import('vitest').Mock).mockResolvedValueOnce({
        rows: [
          {
            sequence: '2',
            timestamp,
            actor_type: 'agent',
            actor_id: 'agent-1',
            event_type: 'test.event',
            payload: { foo: 'bar' },
            prev_hash: 'WRONG-PREV-HASH',
            hash: 'hash-2',
          },
          {
            sequence: '1',
            timestamp,
            actor_type: 'agent',
            actor_id: 'agent-1',
            event_type: 'test.event',
            payload: { foo: 'bar' },
            prev_hash: null,
            hash: 'hash-1',
          },
        ],
      });

      const result = await service.verifyIntegrity();
      expect(result.valid).toBe(false);
      expect(result.brokenAt).toBe('2');

      serviceObj.computeHash = originalComputeHash;
    });
  });
});
