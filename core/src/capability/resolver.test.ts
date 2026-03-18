import { describe, it, expect, beforeEach, vi } from 'vitest';
import { CapabilityResolver } from './resolver.js';

describe('CapabilityResolver', () => {
  let registryMock: any;
  let resolver: CapabilityResolver;

  beforeEach(() => {
    registryMock = {
      getInstance: vi.fn(),
      getTemplate: vi.fn(),
      getSandboxBoundary: vi.fn(),
      getCapabilityPolicy: vi.fn(),
    };
    resolver = new CapabilityResolver(registryMock);
  });

  it('throws error if instance not found', async () => {
    registryMock.getInstance.mockResolvedValue(null);
    await expect(resolver.resolve('id')).rejects.toThrow('Instance id not found');
  });

  it('throws error if template not found', async () => {
    registryMock.getInstance.mockResolvedValue({ template_ref: 'tpl' });
    registryMock.getTemplate.mockResolvedValue(null);
    await expect(resolver.resolve('id')).rejects.toThrow('Template tpl not found');
  });

  it('resolves capabilities by intersecting boundary, policy and manifest', async () => {
    registryMock.getInstance.mockResolvedValue({ 
      template_ref: 'tpl',
      overrides: { capabilities: { network: { outbound: ['google.com'] } } }
    });
    registryMock.getTemplate.mockResolvedValue({ 
      spec: { sandboxBoundary: 'tier-2', policyRef: 'p1' } 
    });
    registryMock.getSandboxBoundary.mockResolvedValue({
      name: 'tier-2',
      capabilities: { network: { outbound: ['google.com', 'github.com'] } }
    });
    registryMock.getCapabilityPolicy.mockResolvedValue({
      name: 'p1',
      capabilities: { network: { outbound: ['google.com', 'github.com'] } }
    });

    const result = await resolver.resolve('id');
    expect(result.resolvedCapabilities.network.outbound).toEqual(['google.com']);
  });

  it('handles missing policy or inline overrides', async () => {
    registryMock.getInstance.mockResolvedValue({ template_ref: 'tpl' });
    registryMock.getTemplate.mockResolvedValue({ spec: { sandboxBoundary: 'b1' } });
    registryMock.getSandboxBoundary.mockResolvedValue({
      capabilities: { network: { outbound: ['a.com'] } }
    });

    const result = await resolver.resolve('id');
    expect(result.resolvedCapabilities.network.outbound).toEqual(['a.com']);
  });

  it('recursive intersection of objects', async () => {
    registryMock.getInstance.mockResolvedValue({ template_ref: 'tpl' });
    registryMock.getTemplate.mockResolvedValue({ spec: { sandboxBoundary: 'b1' } });
    registryMock.getSandboxBoundary.mockResolvedValue({
      capabilities: { fs: { read: true, write: true } }
    });
    registryMock.getInstance.mockResolvedValue({ 
      template_ref: 'tpl',
      overrides: { capabilities: { fs: { write: false } } }
    });

    const result = await resolver.resolve('id');
    expect(result.resolvedCapabilities.fs.read).toBe(true);
    expect(result.resolvedCapabilities.fs.write).toBe(false);
  });

  it('handles boolean true/false in layers', async () => {
    registryMock.getInstance.mockResolvedValue({ template_ref: 'tpl' });
    registryMock.getTemplate.mockResolvedValue({ spec: { sandboxBoundary: 'b1' } });
    registryMock.getSandboxBoundary.mockResolvedValue({ capabilities: { root: true } });
    
    // Boundary true, Policy false -> false
    registryMock.getCapabilityPolicy.mockResolvedValue({ capabilities: { root: false } });
    registryMock.getTemplate.mockResolvedValue({ spec: { sandboxBoundary: 'b1', policyRef: 'p1' } });
    
    let result = await resolver.resolve('id');
    expect(result.resolvedCapabilities.root).toBe(false);
  });
});
