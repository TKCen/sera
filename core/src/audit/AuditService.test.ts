import crypto from 'crypto';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { AuditService } from '../audit/AuditService.js';

// Mock database to simulate different scenarios
const mockRows: any[] = [];
vi.mock('../lib/database.js', () => ({
  query: vi.fn().mockImplementation(async (text, params) => {
    if (text.startsWith('SELECT hash FROM audit_trail')) {
      return { rows: mockRows.length > 0 ? [mockRows[mockRows.length - 1]] : [] };
    }
    if (text.startsWith('INSERT INTO audit_trail')) {
      const [agent_id, action, details, timestamp, previous_hash, hash] = params;
      const entry = { id: mockRows.length + 1, agent_id, action, details: JSON.parse(details), timestamp, previous_hash, hash };
      mockRows.push(entry);
      return { rowCount: 1 };
    }
    if (text.startsWith('SELECT * FROM audit_trail WHERE agent_id = $1 ORDER BY id ASC')) {
      return { rows: [...mockRows] };
    }
    return { rows: [] };
  }),
  initDb: vi.fn().mockResolvedValue(undefined)
}));

describe('AuditService Merkle Chain', () => {
  beforeEach(() => {
    mockRows.length = 0;
  });

  it('should maintain a valid hash chain', async () => {
    const auditService = AuditService.getInstance();
    const agentId = 'test-agent';

    await auditService.record(agentId, 'action1', { key: 'val1' });
    await auditService.record(agentId, 'action2', { key: 'val2' });
    await auditService.record(agentId, 'action3', { key: 'val3' });

    expect(mockRows.length).toBe(3);
    expect(mockRows[0].previous_hash).toBeNull();
    expect(mockRows[1].previous_hash).toBe(mockRows[0].hash);
    expect(mockRows[2].previous_hash).toBe(mockRows[1].hash);

    const result = await auditService.verify(agentId);
    expect(result.valid).toBe(true);
  });

  it('should detect tampering in details', async () => {
    const auditService = AuditService.getInstance();
    const agentId = 'tamper-agent';

    await auditService.record(agentId, 'action1', { data: 'safe' });
    await auditService.record(agentId, 'action2', { data: 'safe' });

    // Tamper with first entry's details
    mockRows[0].details = { data: 'TAMPERED' };

    const result = await auditService.verify(agentId);
    expect(result.valid).toBe(false);
    expect(result.brokenAt).toBe(1);
    expect(result.reason).toContain('Hash mismatch');
  });

  it('should detect broken links in the chain', async () => {
    const auditService = AuditService.getInstance();
    const agentId = 'link-agent';

    await auditService.record(agentId, 'action1', { data: 'safe' });
    await auditService.record(agentId, 'action2', { data: 'safe' });

    // Tamper with second entry's previous_hash
    mockRows[1].previous_hash = 'corrupted_hash';

    const result = await auditService.verify(agentId);
    expect(result.valid).toBe(false);
    expect(result.brokenAt).toBe(2);
    expect(result.reason).toContain('Previous hash mismatch');
  });
});
