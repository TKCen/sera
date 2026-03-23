import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { ResourceImporter } from './importer.service.js';
import type { AgentRegistry } from './registry.service.js';
import path from 'path';
import fs from 'fs/promises';

describe('ResourceImporter', () => {
  let registryMock: {
    upsertNamedList: import('vitest').Mock;
    upsertCapabilityPolicy: import('vitest').Mock;
    upsertSandboxBoundary: import('vitest').Mock;
    upsertTemplate: import('vitest').Mock;
    deleteTemplatesExcept: import('vitest').Mock;
    deleteNamedListsExcept: import('vitest').Mock;
    deleteCapabilityPoliciesExcept: import('vitest').Mock;
    deleteSandboxBoundariesExcept: import('vitest').Mock;
  };
  let importer: ResourceImporter;
  const baseDir = path.join(process.cwd(), 'test-manifests');

  beforeEach(async () => {
    registryMock = {
      upsertNamedList: vi.fn().mockResolvedValue({ status: 'added', name: 'github-apis' }),
      upsertCapabilityPolicy: vi.fn().mockResolvedValue({ status: 'added', name: 'standard' }),
      upsertSandboxBoundary: vi.fn().mockResolvedValue({ status: 'added', name: 'sb1' }),
      upsertTemplate: vi.fn().mockResolvedValue({ status: 'added', name: 't1' }),
      deleteTemplatesExcept: vi.fn().mockResolvedValue({ removed: [], errors: [] }),
      deleteNamedListsExcept: vi.fn().mockResolvedValue({ removed: [], errors: [] }),
      deleteCapabilityPoliciesExcept: vi.fn().mockResolvedValue({ removed: [], errors: [] }),
      deleteSandboxBoundariesExcept: vi.fn().mockResolvedValue({ removed: [], errors: [] }),
    };
    importer = new ResourceImporter(registryMock as unknown as AgentRegistry, baseDir);

    // Create test directories and files
    await fs.mkdir(path.join(baseDir, 'lists', 'network-allowlist'), { recursive: true });
    await fs.mkdir(path.join(baseDir, 'capability-policies'), { recursive: true });
    await fs.mkdir(path.join(baseDir, 'sandbox-boundaries'), { recursive: true });
    await fs.mkdir(path.join(baseDir, 'templates', 'builtin'), { recursive: true });
    await fs.mkdir(path.join(baseDir, 'agents'), { recursive: true });

    await fs.writeFile(
      path.join(baseDir, 'lists', 'network-allowlist', 'github.yaml'),
      `
apiVersion: sera/v1
kind: NamedList
metadata:
  name: github-apis
  type: network-allowlist
entries:
  - api.github.com
`
    );

    await fs.writeFile(
      path.join(baseDir, 'capability-policies', 'default.yaml'),
      `
apiVersion: sera/v1
kind: CapabilityPolicy
metadata:
  name: standard
capabilities:
  filesystem:
    read: true
`
    );
  });

  afterEach(async () => {
    await fs.rm(baseDir, { recursive: true, force: true });
  });

  it('imports valid resources from directory structure', async () => {
    const report = await importer.importAll();

    expect(registryMock.upsertNamedList).toHaveBeenCalledWith(
      expect.objectContaining({
        metadata: expect.objectContaining({ name: 'github-apis' }),
      })
    );
    expect(registryMock.upsertCapabilityPolicy).toHaveBeenCalledWith(
      expect.objectContaining({
        metadata: expect.objectContaining({ name: 'standard' }),
      })
    );
    expect(report.added).toContain('github-apis');
    expect(report.added).toContain('standard');
  });

  it('skips invalid manifests', async () => {
    await fs.writeFile(
      path.join(baseDir, 'capability-policies', 'invalid.yaml'),
      `invalid manifest`
    );

    await importer.importAll();
    // Should still have called for the valid one
    expect(registryMock.upsertCapabilityPolicy).toHaveBeenCalledTimes(1);
  });

  it('imports from agents directory', async () => {
    await fs.mkdir(path.join(baseDir, 'agents', 'my-agent'), { recursive: true });
    await fs.writeFile(
      path.join(baseDir, 'agents', 'my-agent', 'AGENT.yaml'),
      `
apiVersion: sera/v1
kind: Agent
metadata:
  name: my-agent
identity:
  role: bot
  description: test
model:
  provider: openai
  name: gpt-4
spec:
  lifecycle:
    mode: ephemeral
`
    );
    registryMock.upsertTemplate.mockResolvedValue({ status: 'added', name: 'my-agent' });

    const report = await importer.importAll();
    expect(registryMock.upsertTemplate).toHaveBeenCalled();
    expect(report.added).toContain('my-agent');
  });

  it('handles removals', async () => {
    registryMock.deleteTemplatesExcept.mockResolvedValue({ removed: ['old-template'], errors: [] });
    const report = await importer.importAll();
    expect(report.removed).toContain('old-template');
    expect(registryMock.deleteTemplatesExcept).toHaveBeenCalled();
  });
});
