import { describe, it, expect, beforeEach } from 'vitest';
import { IdentityService } from './IdentityService.js';
import type { AgentTokenPayload } from './types.js';

describe('IdentityService', () => {
  const TEST_SECRET = 'test-secret-key-for-unit-tests';
  let service: IdentityService;

  beforeEach(() => {
    service = new IdentityService(TEST_SECRET);
  });

  const samplePayload: Omit<AgentTokenPayload, 'scope' | 'agentName'> = {
    agentId: 'agent-001',
    circleId: 'dev-circle',
    capabilities: ['internet-access', 'file-write'],
  };

  describe('signToken', () => {
    it('should return a non-empty string token', async () => {
      const token = await service.signToken(samplePayload);
      expect(token).toBeTruthy();
      expect(typeof token).toBe('string');
      // JWT has three dot-separated parts
      expect(token.split('.')).toHaveLength(3);
    });

    it('should default scope to agent when not specified', async () => {
      const token = await service.signToken(samplePayload);
      const claims = await service.verifyToken(token);
      expect(claims.scope).toBe('agent');
    });

    it('should include agentName equal to agentId when not provided', async () => {
      const token = await service.signToken(samplePayload);
      const claims = await service.verifyToken(token);
      expect(claims.agentName).toBe('agent-001');
    });

    it('should include provided agentName', async () => {
      const token = await service.signToken({ ...samplePayload, agentName: 'my-agent', scope: 'internal' });
      const claims = await service.verifyToken(token);
      expect(claims.agentName).toBe('my-agent');
      expect(claims.scope).toBe('internal');
    });
  });

  describe('verifyToken', () => {
    it('should round-trip sign and verify a token', async () => {
      const token = await service.signToken(samplePayload);
      const claims = await service.verifyToken(token);

      expect(claims.agentId).toBe('agent-001');
      expect(claims.circleId).toBe('dev-circle');
      expect(claims.capabilities).toEqual(['internet-access', 'file-write']);
      expect(claims.iat).toBeTypeOf('number');
      expect(claims.exp).toBeTypeOf('number');
      expect(claims.exp).toBeGreaterThan(claims.iat);
    });

    it('should reject a tampered token', async () => {
      const token = await service.signToken(samplePayload);
      // Corrupt the signature
      const tampered = token.slice(0, -4) + 'XXXX';
      await expect(service.verifyToken(tampered)).rejects.toThrow();
    });

    it('should reject a token signed with a different secret', async () => {
      const otherService = new IdentityService('different-secret');
      const token = await otherService.signToken(samplePayload);
      await expect(service.verifyToken(token)).rejects.toThrow();
    });

    it('should reject an expired token', async () => {
      // Sign with 1-second expiry
      const token = await service.signToken(samplePayload, '1s');
      // Wait for expiry
      await new Promise(resolve => setTimeout(resolve, 1100));
      await expect(service.verifyToken(token)).rejects.toThrow();
    });
  });

  describe('constructor fallback', () => {
    it('should use JWT_SECRET from env if provided', async () => {
      const origEnv = process.env.JWT_SECRET;
      process.env.JWT_SECRET = 'env-secret';
      try {
        const envService = new IdentityService();
        const token = await envService.signToken(samplePayload);
        // Verify with the same env-derived secret
        const verifier = new IdentityService('env-secret');
        const claims = await verifier.verifyToken(token);
        expect(claims.agentId).toBe('agent-001');
      } finally {
        if (origEnv !== undefined) {
          process.env.JWT_SECRET = origEnv;
        } else {
          delete process.env.JWT_SECRET;
        }
      }
    });

    it('should generate a random secret when no JWT_SECRET is set', async () => {
      const origEnv = process.env.JWT_SECRET;
      delete process.env.JWT_SECRET;
      try {
        const randomService = new IdentityService();
        const token = await randomService.signToken(samplePayload);
        // Should still be able to verify with the same instance
        const claims = await randomService.verifyToken(token);
        expect(claims.agentId).toBe('agent-001');
      } finally {
        if (origEnv !== undefined) {
          process.env.JWT_SECRET = origEnv;
        }
      }
    });
  });
});
