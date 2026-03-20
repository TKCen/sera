import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { ResourceImporter } from './importer.service.js';
import type { AgentRegistry } from './registry.service.js';
import path from 'path';
import fs from 'fs/promises';

describe('ResourceImporter', () => {
  let registryMock: Record<string, import('vitest').Mock>;
  let importer: ResourceImporter;
  const baseDir = path.join(process.cwd(), 'test-manifests');

  beforeEach(async () => {
    registryMock = {
      upsertNamedList: vi.fn().mockResolvedValue({}),
      upsertCapabilityPolicy: vi.fn().mockResolvedValue({}),
      upsertSandboxBoundary: vi.fn().mockResolvedValue({}),
      upsertTemplate: vi.fn().mockResolvedValue({}),
    };
    importer = new ResourceImporter(registryMock as unknown as AgentRegistry, baseDir);

    // Create test directories and files
    await fs.mkdir(path.join(baseDir, 'lists', 'network-allowlist'), { recursive: true });
    await fs.mkdir(path.join(baseDir, 'capability-policies'), { recursive: true });
    await fs.mkdir(path.join(baseDir, 'sandbox-boundaries'), { recursive: true });
    await fs.mkdir(path.join(baseDir, 'templates', 'builtin'), { recursive: true });

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
    await importer.importAll();

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
});
