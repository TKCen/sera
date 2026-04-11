import { describe, it, expect, beforeEach, vi } from 'vitest';
import { CapabilityResolver } from './resolver.js';
import type { AgentRegistry } from '../agents/registry.service.js';

describe('CapabilityResolver', () => {
  let registryMock: Record<string, import('vitest').Mock>;
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
    resolver = new CapabilityResolver(registryMock as unknown as AgentRegistry);
  });

  it('throws error if instance not found', async () => {
    registryMock['getInstance']!.mockResolvedValue(null);
    await expect(resolver.resolve('id')).rejects.toThrow(/instance id not found/i);
  });

  it('throws error if template not found', async () => {
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });
    registryMock['getTemplate']!.mockResolvedValue(null);
    await expect(resolver.resolve('id')).rejects.toThrow(/template tpl not found/i);
  });

  it('throws error if boundary not found', async () => {
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getSandboxBoundary']!.mockResolvedValue(null);
    await expect(resolver.resolve('id')).rejects.toThrow(/boundary b1 not found/i);
  });

  it('resolves capabilities by intersecting boundary, policy and manifest', async () => {
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: { capabilities: { network: { outbound: ['google.com'] } } },
    });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'tier-2', policyRef: 'p1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getSandboxBoundary']!.mockResolvedValue({
      name: 'tier-2',
      capabilities: { network: { outbound: ['google.com', 'github.com'] } },
    });
    registryMock['getCapabilityPolicy']!.mockResolvedValue({
      name: 'p1',
      capabilities: { network: { outbound: ['google.com', 'github.com'] } },
    });

    const result = await resolver.resolve('id');
    const caps = result.resolvedCapabilities as Record<string, unknown>;
    expect((caps?.network as Record<string, unknown>)?.outbound).toEqual(['google.com']);
  });

  it('handles missing policy or inline overrides', async () => {
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getSandboxBoundary']!.mockResolvedValue({
      capabilities: { network: { outbound: ['a.com'] } },
    });

    const result = await resolver.resolve('id');
    const caps = result.resolvedCapabilities as Record<string, unknown>;
    expect((caps?.network as Record<string, unknown>)?.outbound).toEqual(['a.com']);
  });

  it('recursive intersection of objects and narrowing', async () => {
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: { capabilities: { fs: { write: false } } },
    });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getSandboxBoundary']!.mockResolvedValue({
      capabilities: { fs: { read: true, write: true } },
    });

    const result = await resolver.resolve('id');
    const caps = result.resolvedCapabilities as Record<string, unknown>;
    expect((caps?.fs as Record<string, unknown>)?.read).toBe(true);
    expect((caps?.fs as Record<string, unknown>)?.write).toBe(false);
  });

  it('handles boolean true/false in layers', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: { root: true } });
    registryMock['getCapabilityPolicy']!.mockResolvedValue({ capabilities: { root: false } });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', policyRef: 'p1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });

    const result = await resolver.resolve('id');
    const caps = result.resolvedCapabilities as Record<string, unknown>;
    expect(caps?.root).toBe(false);
  });

  it('throws CapabilityEscalationError on broadening', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({
      capabilities: { fs: { read: true, write: false } },
    });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: { capabilities: { fs: { write: true } } },
    });

    await expect(resolver.resolve('id')).rejects.toThrow(/Capability escalation detected/i);
  });

  it('resolves NamedList references recursively', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({
      capabilities: { network: { outbound: ['google.com', 'github.com', 'npm.org'] } },
    });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: {
        sandboxBoundary: 'b1',
        capabilities: { network: { outbound: [{ $ref: 'list1' }] } },
        lifecycle: { mode: 'persistent' },
      },
    });
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });

    registryMock['getNamedList']!.mockImplementation((name: string) => {
      if (name === 'list1') return { entries: ['google.com', { $ref: 'list2' }] };
      if (name === 'list2') return { entries: ['github.com'] };
      return null;
    });

    const result = await resolver.resolve('id');
    const caps = result.resolvedCapabilities as Record<string, unknown>;
    expect((caps?.network as Record<string, unknown>)?.outbound).toEqual([
      'google.com',
      'github.com',
    ]);
  });

  it('detects circular references in NamedLists', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({
      capabilities: { network: { outbound: ['a.com'] } },
    });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: {
        sandboxBoundary: 'b1',
        capabilities: { network: { outbound: [{ $ref: 'l1' }] } },
        lifecycle: { mode: 'persistent' },
      },
    });
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });

    registryMock['getNamedList']!.mockImplementation((name: string) => {
      if (name === 'l1') return { entries: [{ $ref: 'l2' }] };
      if (name === 'l2') return { entries: [{ $ref: 'l1' }] };
      return null;
    });

    await expect(resolver.resolve('id')).rejects.toThrow(/circular reference detected/i);
  });

  it('enforces always-denied lists', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({
      capabilities: { exec: { commands: ['git *', 'rm -rf /'] } },
    });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });

    registryMock['listAlwaysEnforcedNamedLists']!.mockResolvedValue([
      {
        type: 'command-denylist',
        entries: ['rm -rf *'],
      },
    ]);

    const result = await resolver.resolve('id');
    const caps = result.resolvedCapabilities as Record<string, unknown>;
    expect((caps?.exec as Record<string, unknown>)?.commands).toEqual(['git *']);
  });

  it('handles broadening of arrays (escalation)', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: { net: ['a.com'] } });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: { capabilities: { net: ['a.com', 'b.com'] } },
    });

    await expect(resolver.resolve('id')).rejects.toThrow(/Capability escalation detected/i);
  });

  it('handles mismatch in capability types (escalation)', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: { net: ['a.com'] } });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: { capabilities: { net: { allow: ['a.com'] } } },
    });

    await expect(resolver.resolve('id')).rejects.toThrow(/Capability escalation detected/i);
  });

  it('throws CapabilityEscalationError when boundary is object but manifest is scalar', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: { fs: { read: true } } });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: { capabilities: { fs: true } },
    });

    await expect(resolver.resolve('id')).rejects.toThrow(/Capability escalation detected/i);
  });

  it('allows empty array or object in manifest even if undefined in boundary', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: { fs: { read: true } } });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: { capabilities: { fs: { write: [] }, net: {} } },
    });

    const result = await resolver.resolve('id');
    const caps = result.resolvedCapabilities as Record<string, unknown>;
    expect((caps?.fs as Record<string, unknown>)?.write).toBe(false);
    expect(caps?.net).toBe(false);
  });

  it('skips unknown list types in always-denied enforcement', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: { root: true } });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: { sandboxBoundary: 'b1', lifecycle: { mode: 'persistent' } },
    });
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });

    registryMock['listAlwaysEnforcedNamedLists']!.mockResolvedValue([
      {
        type: 'unknown-type',
        entries: ['something'],
      },
    ]);

    const result = await resolver.resolve('id');
    const caps = result.resolvedCapabilities as Record<string, unknown>;
    expect(caps?.root).toBe(true);
  });

  it('throws error if NamedList reference not found', async () => {
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: { net: ['*'] } });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: {
        sandboxBoundary: 'b1',
        capabilities: { net: [{ $ref: 'missing' }] },
        lifecycle: { mode: 'persistent' },
      },
    });
    registryMock['getInstance']!.mockResolvedValue({ template_ref: 'tpl' });
    registryMock['getNamedList']!.mockResolvedValue(null);

    await expect(resolver.resolve('id')).rejects.toThrow(/NamedList missing not found/i);
  });

  it('deepMerge handles $append and $remove for skills', async () => {
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: {
        skills: {
          $append: ['new-skill'],
          $remove: ['old-skill'],
        },
      },
    });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: {
        sandboxBoundary: 'b1',
        skills: ['old-skill', 'base-skill'],
        lifecycle: { mode: 'persistent' },
      },
    });
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: {} });

    const result = await resolver.resolve('id');
    const spec = result.spec as Record<string, unknown>;
    expect(spec?.skills).toContain('base-skill');
    expect(spec?.skills).toContain('new-skill');
    expect(spec?.skills).not.toContain('old-skill');
  });

  it('deepMerge handles scalar $append/$remove', async () => {
    registryMock['getInstance']!.mockResolvedValue({
      template_ref: 'tpl',
      overrides: {
        skills: {
          $append: 'scalar-skill',
          $remove: 'base-skill',
        },
      },
    });
    registryMock['getTemplate']!.mockResolvedValue({
      spec: {
        sandboxBoundary: 'b1',
        skills: ['base-skill'],
        lifecycle: { mode: 'persistent' },
      },
    });
    registryMock['getSandboxBoundary']!.mockResolvedValue({ capabilities: {} });

    const result = await resolver.resolve('id');
    const spec = result.spec as Record<string, unknown>;
    expect(spec?.skills).toEqual(['scalar-skill']);
  });
});
