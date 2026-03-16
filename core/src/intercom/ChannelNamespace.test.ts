import { describe, it, expect } from 'vitest';
import { ChannelNamespace } from './ChannelNamespace.js';

describe('ChannelNamespace', () => {
  // ── Builder Tests ───────────────────────────────────────────────────────────

  describe('builders', () => {
    it('builds a thoughts channel', () => {
      expect(ChannelNamespace.thoughts('architect-prime'))
        .toBe('internal:agent:architect-prime:thoughts');
    });

    it('builds a terminal channel', () => {
      expect(ChannelNamespace.terminal('developer-prime'))
        .toBe('internal:agent:developer-prime:terminal');
    });

    it('builds a DM channel with sorted agent names', () => {
      // Alphabetical order should be consistent regardless of argument order
      expect(ChannelNamespace.dm('development', 'developer-prime', 'architect-prime'))
        .toBe('intercom:development:architect-prime:developer-prime');
      expect(ChannelNamespace.dm('development', 'architect-prime', 'developer-prime'))
        .toBe('intercom:development:architect-prime:developer-prime');
    });

    it('builds a circle channel', () => {
      expect(ChannelNamespace.circleChannel('development', 'architecture-decisions'))
        .toBe('channel:development:architecture-decisions');
    });

    it('builds a bridge channel with sorted circle names', () => {
      expect(ChannelNamespace.bridge('operations', 'development', 'deployment-requests'))
        .toBe('bridge:development:operations:deployment-requests');
      expect(ChannelNamespace.bridge('development', 'operations', 'deployment-requests'))
        .toBe('bridge:development:operations:deployment-requests');
    });

    it('builds a status channel', () => {
      expect(ChannelNamespace.status('architect-prime'))
        .toBe('public:status:architect-prime');
    });

    it('builds an external inbox channel', () => {
      expect(ChannelNamespace.externalInbox('mobile-app-123'))
        .toBe('external:mobile-app-123:inbox');
    });
  });

  // ── Validation Tests ────────────────────────────────────────────────────────

  describe('validate', () => {
    it('validates internal thoughts channel', () => {
      expect(ChannelNamespace.validate('internal:agent:architect-prime:thoughts'))
        .toBe('internal');
    });

    it('validates internal terminal channel', () => {
      expect(ChannelNamespace.validate('internal:agent:developer-prime:terminal'))
        .toBe('internal');
    });

    it('validates intercom DM channel', () => {
      expect(ChannelNamespace.validate('intercom:development:architect-prime:developer-prime'))
        .toBe('intercom');
    });

    it('validates circle channel', () => {
      expect(ChannelNamespace.validate('channel:development:architecture-decisions'))
        .toBe('channel');
    });

    it('validates bridge channel', () => {
      expect(ChannelNamespace.validate('bridge:development:operations:deployment-requests'))
        .toBe('bridge');
    });

    it('validates public status channel', () => {
      expect(ChannelNamespace.validate('public:status:architect-prime'))
        .toBe('public');
    });

    it('validates external inbox channel', () => {
      expect(ChannelNamespace.validate('external:mobile-app-123:inbox'))
        .toBe('external');
    });

    it('rejects empty string', () => {
      expect(ChannelNamespace.validate('')).toBeNull();
    });

    it('rejects unknown prefix', () => {
      expect(ChannelNamespace.validate('foobar:something:else')).toBeNull();
    });

    it('rejects internal channel with wrong subtype', () => {
      expect(ChannelNamespace.validate('internal:agent:architect-prime:logs')).toBeNull();
    });

    it('rejects internal channel with wrong structure', () => {
      expect(ChannelNamespace.validate('internal:agent:thoughts')).toBeNull();
    });

    it('rejects intercom channel with too few segments', () => {
      expect(ChannelNamespace.validate('intercom:development:architect-prime')).toBeNull();
    });

    it('rejects channel with uppercase characters', () => {
      expect(ChannelNamespace.validate('internal:agent:ArchitectPrime:thoughts')).toBeNull();
    });

    it('rejects channel with spaces', () => {
      expect(ChannelNamespace.validate('internal:agent:architect prime:thoughts')).toBeNull();
    });

    it('rejects public channel without status subtype', () => {
      expect(ChannelNamespace.validate('public:health:architect-prime')).toBeNull();
    });

    it('rejects external channel without inbox suffix', () => {
      expect(ChannelNamespace.validate('external:sub-123:messages')).toBeNull();
    });
  });

  // ── isValid ─────────────────────────────────────────────────────────────────

  describe('isValid', () => {
    it('returns true for valid channels', () => {
      expect(ChannelNamespace.isValid('internal:agent:test-agent:thoughts')).toBe(true);
    });

    it('returns false for invalid channels', () => {
      expect(ChannelNamespace.isValid('invalid:channel')).toBe(false);
    });
  });

  // ── getPrefix ───────────────────────────────────────────────────────────────

  describe('getPrefix', () => {
    it('extracts the prefix', () => {
      expect(ChannelNamespace.getPrefix('internal:agent:test:thoughts')).toBe('internal');
    });

    it('returns the full string when no colon exists', () => {
      expect(ChannelNamespace.getPrefix('nocolon')).toBe('nocolon');
    });
  });
});
