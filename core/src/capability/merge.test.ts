import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CapabilityResolver } from './resolver.js';

describe('CapabilityResolver - deepMerge $append/$remove', () => {
  let registryMock: any;
  let resolver: CapabilityResolver;

  beforeEach(() => {
    registryMock = {
      getInstance: vi.fn(),
      getTemplate: vi.fn(),
      getSandboxBoundary: vi.fn(),
      getCapabilityPolicy: vi.fn(),
      getNamedList: vi.fn(),
    };
    resolver = new CapabilityResolver(registryMock);
  });

  it('supports $append for skills', async () => {
    const base = { skills: ['s1', 's2'] };
    const overrides = { skills: { $append: ['s3', 's1'] } };
    const result = (resolver as any).deepMerge(base, overrides);
    expect(result.skills).toEqual(['s1', 's2', 's3']);
  });

  it('supports $remove for skills', async () => {
    const base = { skills: ['s1', 's2', 's3'] };
    const overrides = { skills: { $remove: ['s2'] } };
    const result = (resolver as any).deepMerge(base, overrides);
    expect(result.skills).toEqual(['s1', 's3']);
  });

  it('supports both $append and $remove', async () => {
    const base = { skills: ['s1', 's2'] };
    const overrides = { skills: { $append: ['s3'], $remove: ['s1'] } };
    const result = (resolver as any).deepMerge(base, overrides);
    expect(result.skills).toEqual(['s2', 's3']);
  });
});
