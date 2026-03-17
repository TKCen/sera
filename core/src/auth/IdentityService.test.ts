import { describe, it, expect, vi, beforeEach } from 'vitest';
import { IdentityService } from './IdentityService.js';
import type { AgentTokenPayload } from './types.js';

describe('IdentityService', () => {
  const TEST_SECRET = 'test-secret-key-for-unit-tests';
  let service: IdentityService;

  beforeEach(() => {
    service = new IdentityService(TEST_SECRET);
  });

  const samplePayload: AgentTokenPayload = {
    agentId: 'agent-001',
    circleId: 'dev-circle',
    capabilities: ['internet-access', 'file-write'],
  };

  describe('signToken', () => {
    it('should return a non-empty string token', () => {
      const token = service.signToken(samplePayload);
      expect(token).toBeTruthy();
      expect(typeof token).toBe('string');
      // JWT has three dot-separated parts
      expect(token.split('.')).toHaveLength(3);
    });
  });

  describe('verifyToken', () => {
    it('should round-trip sign and verify a token', () => {
      const token = service.signToken(samplePayload);
      const claims = service.verifyToken(token);

      expect(claims.agentId).toBe('agent-001');
      expect(claims.circleId).toBe('dev-circle');
      expect(claims.capabilities).toEqual(['internet-access', 'file-write']);
      expect(claims.iat).toBeTypeOf('number');
      expect(claims.exp).toBeTypeOf('number');
      expect(claims.exp).toBeGreaterThan(claims.iat);
    });

    it('should reject a tampered token', () => {
      const token = service.signToken(samplePayload);
      // Corrupt the signature
      const tampered = token.slice(0, -4) + 'XXXX';
      expect(() => service.verifyToken(tampered)).toThrow();
    });

    it('should reject a token signed with a different secret', () => {
      const otherService = new IdentityService('different-secret');
      const token = otherService.signToken(samplePayload);
      expect(() => service.verifyToken(token)).toThrow();
    });

    it('should reject an expired token', async () => {
      // Sign with 0-second expiry
      const token = service.signToken(samplePayload, '0s');
      // Small delay to ensure expiry
      await new Promise(resolve => setTimeout(resolve, 50));
      expect(() => service.verifyToken(token)).toThrow();
    });
  });

  describe('constructor fallback', () => {
    it('should use JWT_SECRET from env if provided', () => {
      const origEnv = process.env.JWT_SECRET;
      process.env.JWT_SECRET = 'env-secret';
      try {
        const envService = new IdentityService();
        const token = envService.signToken(samplePayload);
        // Verify with the same env-derived secret
        const verifier = new IdentityService('env-secret');
        const claims = verifier.verifyToken(token);
        expect(claims.agentId).toBe('agent-001');
      } finally {
        if (origEnv !== undefined) {
          process.env.JWT_SECRET = origEnv;
        } else {
          delete process.env.JWT_SECRET;
        }
      }
    });

    it('should generate a random secret when no JWT_SECRET is set', () => {
      const origEnv = process.env.JWT_SECRET;
      delete process.env.JWT_SECRET;
      try {
        const randomService = new IdentityService();
        const token = randomService.signToken(samplePayload);
        // Should still be able to verify with the same instance
        const claims = randomService.verifyToken(token);
        expect(claims.agentId).toBe('agent-001');
      } finally {
        if (origEnv !== undefined) {
          process.env.JWT_SECRET = origEnv;
        }
      }
    });
  });
});
