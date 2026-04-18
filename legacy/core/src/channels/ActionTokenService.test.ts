import { describe, it, expect, beforeEach } from 'vitest';
import { ActionTokenService } from './ActionTokenService.js';

describe('ActionTokenService', () => {
  let svc: ActionTokenService;

  beforeEach(() => {
    // Reset singleton between tests
    (ActionTokenService as unknown as { instance: undefined }).instance = undefined;
    svc = ActionTokenService.getInstance();
  });

  describe('issue()', () => {
    it('returns approve and deny tokens with an expiry', async () => {
      const { approveToken, denyToken, expiresAt } = await svc.issue('req-1', 'permission');

      expect(typeof approveToken).toBe('string');
      expect(typeof denyToken).toBe('string');
      expect(approveToken).not.toBe(denyToken);
      expect(new Date(expiresAt).getTime()).toBeGreaterThan(Date.now());
    });
  });

  describe('verify()', () => {
    it('verifies a valid approve token', async () => {
      const { approveToken } = await svc.issue('req-abc', 'permission');
      const claims = await svc.verify(approveToken);

      expect(claims.sub).toBe('req-abc');
      expect(claims.action).toBe('approve');
      expect(claims.requestType).toBe('permission');
      expect(claims.iss).toBe('sera');
    });

    it('verifies a valid deny token', async () => {
      const { denyToken } = await svc.issue('req-abc', 'delegation');
      const claims = await svc.verify(denyToken);

      expect(claims.action).toBe('deny');
      expect(claims.requestType).toBe('delegation');
    });

    it('throws on an invalid signature', async () => {
      const { approveToken } = await svc.issue('req-x', 'permission');
      const tampered = approveToken.slice(0, -5) + 'XXXXX';
      await expect(svc.verify(tampered)).rejects.toThrow();
    });

    it('throws on a malformed token', async () => {
      await expect(svc.verify('not.a.jwt')).rejects.toThrow();
    });

    it('tokens expire after TTL (mocked clock)', async () => {
      // We cannot easily advance time in unit tests without a fake timer
      // but we CAN verify the exp claim is ~15 min from now
      const before = Math.floor(Date.now() / 1000);
      const { approveToken } = await svc.issue('req-y', 'permission');
      const claims = await svc.verify(approveToken);
      const after = Math.floor(Date.now() / 1000);

      const expectedMin = before + 14 * 60;
      const expectedMax = after + 15 * 60 + 1;

      expect(claims.exp).toBeGreaterThanOrEqual(expectedMin);
      expect(claims.exp).toBeLessThanOrEqual(expectedMax);
    });
  });

  describe('buildActionUrls()', () => {
    it('embeds tokens in the URL', async () => {
      const { approveToken, denyToken } = await svc.issue('req-z', 'permission');
      const { approveUrl, denyUrl } = svc.buildActionUrls(approveToken, denyToken);

      expect(approveUrl).toContain('/api/notifications/action');
      expect(approveUrl).toContain(encodeURIComponent(approveToken));
      expect(denyUrl).toContain(encodeURIComponent(denyToken));
    });
  });
});
