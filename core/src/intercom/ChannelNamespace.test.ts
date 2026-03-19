import { describe, it, expect } from 'vitest';
import { ChannelNamespace } from './ChannelNamespace.js';

describe('ChannelNamespace', () => {
  // ── Builder Tests ───────────────────────────────────────────────────────────

  describe('builders', () => {
    it('builds a thoughts channel', () => {
      expect(ChannelNamespace.thoughts('architect-prime'))
        .toBe('thoughts:architect-prime');
    });

    it('builds a tokens channel', () => {
      expect(ChannelNamespace.tokens('developer-prime'))
        .toBe('tokens:developer-prime');
    });

    it('builds a DM channel with sorted agent IDs', () => {
      // Alphabetical order should be consistent regardless of argument order
      expect(ChannelNamespace.private('developer-prime', 'architect-prime'))
        .toBe('private:architect-prime:developer-prime');
      expect(ChannelNamespace.private('architect-prime', 'developer-prime'))
        .toBe('private:architect-prime:developer-prime');
    });

    it('builds a circle channel', () => {
      expect(ChannelNamespace.circle('development'))
        .toBe('circle:development');
    });

    it('builds a status channel', () => {
      expect(ChannelNamespace.status('architect-prime'))
        .toBe('agent:architect-prime:status');
    });

    it('builds a system channel', () => {
      expect(ChannelNamespace.system('agents'))
        .toBe('system.agents');
    });
  });

  // ── Validation Tests ────────────────────────────────────────────────────────

  describe('validate', () => {
    it('validates thoughts channel', () => {
      expect(ChannelNamespace.validate('thoughts:architect-prime'))
        .toBe('thoughts');
    });

    it('validates tokens channel', () => {
      expect(ChannelNamespace.validate('tokens:developer-prime'))
        .toBe('tokens');
    });

    it('validates private DM channel', () => {
      expect(ChannelNamespace.validate('private:architect-prime:developer-prime'))
        .toBe('private');
    });

    it('validates circle channel', () => {
      expect(ChannelNamespace.validate('circle:development'))
        .toBe('circle');
    });

    it('validates status channel', () => {
      expect(ChannelNamespace.validate('agent:architect-prime:status'))
        .toBe('agent');
    });

    it('validates system channel', () => {
      expect(ChannelNamespace.validate('system.agents'))
        .toBe('system');
    });

    it('rejects empty string', () => {
      expect(ChannelNamespace.validate('')).toBeNull();
    });

    it('rejects unknown prefix', () => {
      expect(ChannelNamespace.validate('foobar:something:else')).toBeNull();
    });

    it('rejects status channel with wrong suffix', () => {
      expect(ChannelNamespace.validate('agent:architect-prime:logs')).toBeNull();
    });

    it('rejects thoughts channel with wrong structure', () => {
      expect(ChannelNamespace.validate('thoughts:agent:architect-prime')).toBeNull();
    });

    it('rejects private channel with too few segments', () => {
      expect(ChannelNamespace.validate('private:architect-prime')).toBeNull();
    });

    it('rejects channel with uppercase characters', () => {
      expect(ChannelNamespace.validate('thoughts:ArchitectPrime')).toBeNull();
    });

    it('rejects channel with spaces', () => {
      expect(ChannelNamespace.validate('thoughts:architect prime')).toBeNull();
    });
  });

  // ── isValid ─────────────────────────────────────────────────────────────────

  describe('isValid', () => {
    it('returns true for valid channels', () => {
      expect(ChannelNamespace.isValid('thoughts:test-agent')).toBe(true);
    });

    it('returns false for invalid channels', () => {
      expect(ChannelNamespace.isValid('invalid:channel')).toBe(false);
    });
  });

  // ── getPrefix ───────────────────────────────────────────────────────────────

  describe('getPrefix', () => {
    it('extracts the prefix', () => {
      expect(ChannelNamespace.getPrefix('thoughts:test')).toBe('thoughts');
    });

    it('extracts system prefix', () => {
      expect(ChannelNamespace.getPrefix('system.agents')).toBe('system');
    });
  });
});
