import { describe, it, expect, vi, beforeEach } from 'vitest';
import { PostgresSecretsProvider } from './postgres-secrets-provider.js';
import * as db from '../lib/database.js';

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

describe('PostgresSecretsProvider', () => {
  const MASTER_KEY = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'; // 32 bytes hex
  
  beforeEach(() => {
    vi.clearAllMocks();
    process.env.SECRETS_MASTER_KEY = MASTER_KEY;
  });

  describe('constructor', () => {
    it('should throw if SECRETS_MASTER_KEY is missing', () => {
      delete process.env.SECRETS_MASTER_KEY;
      expect(() => new PostgresSecretsProvider()).toThrow('SECRETS_MASTER_KEY not set');
    });

    it('should throw if SECRETS_MASTER_KEY is not 32 bytes', () => {
      process.env.SECRETS_MASTER_KEY = 'too-short';
      expect(() => new PostgresSecretsProvider()).toThrow('SECRETS_MASTER_KEY must be a 32-byte hex string');
    });
  });

  describe('encryption/decryption round-trip', () => {
    it('should encrypt and decrypt a value correctly', async () => {
      const provider = new PostgresSecretsProvider();
      const secretName = 'test-secret';
      const secretValue = 'super-secret-password-123';
      const agentContext = { agentId: 'agent-1', agentName: 'test-agent' };

      // Mock database response for the get call
      let storedValue: any;
      let storedIv: any;

      vi.mocked(db.query).mockImplementation(async (sql, params) => {
        if (sql.includes('INSERT INTO secrets')) {
          storedValue = params?.[1];
          storedIv = params?.[2];
          return { rowCount: 1, rows: [] } as any;
        }
        if (sql.includes('SELECT encrypted_value')) {
          return {
            rowCount: 1,
            rows: [{
              encrypted_value: storedValue,
              iv: storedIv,
              allowed_agents: ['test-agent']
            }]
          } as any;
        }
        return { rowCount: 0, rows: [] } as any;
      });

      await provider.set(secretName, secretValue, { allowedAgents: ['test-agent'] });
      const retrievedValue = await provider.get(secretName, agentContext);

      expect(retrievedValue).toBe(secretValue);
      expect(db.query).toHaveBeenCalledTimes(2);
    });

    it('should throw if decryption fails (e.g. tampered data)', async () => {
      const provider = new PostgresSecretsProvider();
      const secretName = 'tampered-secret';
      const agentContext = { agentId: 'agent-1', agentName: 'test-agent' };

      vi.mocked(db.query).mockResolvedValue({
        rowCount: 1,
        rows: [{
          encrypted_value: Buffer.from('invalid-data'),
          iv: Buffer.alloc(12),
          allowed_agents: ['test-agent']
        }]
      } as any);

      await expect(provider.get(secretName, agentContext)).rejects.toThrow('Secret decryption failed');
    });
  });

  describe('authorization', () => {
    it('should return null if agent is not allowed', async () => {
      const provider = new PostgresSecretsProvider();
      const secretName = 'restricted-secret';
      const agentContext = { agentId: 'agent-1', agentName: 'malicious-agent' };

      vi.mocked(db.query).mockResolvedValue({
        rowCount: 1,
        rows: [{
          encrypted_value: Buffer.from('...'),
          iv: Buffer.alloc(12),
          allowed_agents: ['authorized-agent']
        }]
      } as any);

      const value = await provider.get(secretName, agentContext);
      expect(value).toBeNull();
    });

    it('should allow access if "*" is in allowed_agents', async () => {
      const provider = new PostgresSecretsProvider();
      
      // We need to use the real internal encrypt to get a valid ciphertext
      const secretValue = 'unrestricted-data';
      
      // Accessing private method for testing encryption logic
      const { encryptedValue, iv } = (provider as any).encrypt(secretValue);

      vi.mocked(db.query).mockResolvedValue({
        rowCount: 1,
        rows: [{
          encrypted_value: encryptedValue,
          iv: iv,
          allowed_agents: ['*']
        }]
      } as any);

      const value = await provider.get('any-secret', { agentId: 'any', agentName: 'any' });
      expect(value).toBe(secretValue);
    });
  });

  describe('list and delete', () => {
    it('should list secrets with metadata', async () => {
      const provider = new PostgresSecretsProvider();
      const mockResult = {
        rowCount: 1,
        rows: [{
          id: '1',
          name: 'secret1',
          allowed_agents: ['agent1'],
          tags: ['prod'],
          exposure: 'per-call',
          created_at: new Date(),
          updated_at: new Date()
        }]
      };
      vi.mocked(db.query).mockResolvedValue(mockResult as any);

      const list = await provider.list({ tags: ['prod'] });
      expect(list).toHaveLength(1);
      expect(list[0].name).toBe('secret1');
      expect(db.query).toHaveBeenCalledWith(expect.stringContaining('tags && $1'), [['prod']]);
    });

    it('should perform soft delete', async () => {
      const provider = new PostgresSecretsProvider();
      await provider.delete('old-secret');
      expect(db.query).toHaveBeenCalledWith(expect.stringContaining('UPDATE secrets SET deleted_at = NOW()'), ['old-secret']);
    });
  });

  describe('healthCheck', () => {
    it('should return true if DB is healthy', async () => {
      const provider = new PostgresSecretsProvider();
      vi.mocked(db.query).mockResolvedValue({} as any);
      expect(await provider.healthCheck()).toBe(true);
    });

    it('should return false if DB is unhealthy', async () => {
      const provider = new PostgresSecretsProvider();
      vi.mocked(db.query).mockRejectedValue(new Error('DB Down'));
      expect(await provider.healthCheck()).toBe(false);
    });
  });
});
