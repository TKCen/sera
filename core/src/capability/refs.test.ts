import { describe, it, expect, beforeEach, vi } from 'vitest';
import { CapabilityResolver } from './resolver.js';

describe('CapabilityResolver - NamedList & $ref', () => {
  let registryMock: any;
  let resolver: CapabilityResolver;

  beforeEach(() => {
    registryMock = {
      getInstance: vi.fn(),
      getTemplate: vi.fn(),
      getSandboxBoundary: vi.fn(),
      getCapabilityPolicy: vi.fn(),
      getNamedList: vi.fn(),
      listAlwaysEnforcedNamedLists: vi.fn().mockResolvedValue([]),
    };
    resolver = new CapabilityResolver(registryMock);

    registryMock.getInstance.mockResolvedValue({ template_ref: 'tpl' });
    registryMock.getTemplate.mockResolvedValue({ spec: { sandboxBoundary: 'b1' } });
  });

  it('expands $ref in NamedLists', async () => {
    registryMock.getSandboxBoundary.mockResolvedValue({
      capabilities: { 'network-allowlist': [{ $ref: 'base-apis' }] },
    });
    registryMock.getNamedList.mockResolvedValue({
      entries: ['api.github.com', 'api.openai.com'],
    });

    const result = await resolver.resolve('id');
    expect(result.resolvedCapabilities['network-allowlist']).toContain('api.github.com');
  });

  it('detects circular references', async () => {
    registryMock.getSandboxBoundary.mockResolvedValue({
      capabilities: { 'network-allowlist': [{ $ref: 'list-a' }] },
    });
    // list-a -> list-b -> list-a
    // list-a -> list-b -> list-a
    registryMock.getNamedList.mockImplementation((name: string) => {
      if (name === 'list-a') return Promise.resolve({ entries: [{ $ref: 'list-b' }] });
      if (name === 'list-b') return Promise.resolve({ entries: [{ $ref: 'list-a' }] });
      return Promise.resolve(null);
    });

    await expect(resolver.resolve('id')).rejects.toThrow('Circular reference detected');
  });

  it('intersects expanded references', async () => {
    // Boundary: github + openai
    // Policy: github + anthropic
    // Result: github
    registryMock.getSandboxBoundary.mockResolvedValue({
      capabilities: { 'network-allowlist': [{ $ref: 'boundary-list' }] },
    });
    registryMock.getCapabilityPolicy.mockResolvedValue({
      capabilities: { 'network-allowlist': [{ $ref: 'policy-list' }] },
    });
    registryMock.getTemplate.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', policyRef: 'p1' },
    });

    registryMock.getNamedList.mockImplementation((name: string) => {
      if (name === 'boundary-list')
        return Promise.resolve({ entries: ['github.com', 'openai.com'] });
      if (name === 'policy-list')
        return Promise.resolve({ entries: ['github.com', 'anthropic.com'] });
      return Promise.resolve(null);
    });

    const result = await resolver.resolve('id');
    expect(result.resolvedCapabilities['network-allowlist']).toEqual(['github.com']);
  });
});
