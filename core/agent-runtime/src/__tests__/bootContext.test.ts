import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import { loadBootContext } from '../bootContext.js';
import type { RuntimeManifest } from '../manifest.js';

vi.mock('fs');
vi.mock('./logger.js', () => ({
  log: vi.fn(),
}));

describe('loadBootContext', () => {
  const workspacePath = '/workspace';
  const mockManifest: RuntimeManifest = {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: { name: 'test-agent', displayName: 'Test Agent', icon: '', circle: '', tier: 1 },
    identity: { role: 'tester', description: 'testing' },
    model: { provider: 'openai', name: 'gpt-4' },
    bootContext: {
      files: [
        { path: 'test.md', label: 'Test File' }
      ]
    }
  };

  beforeEach(() => {
    vi.resetAllMocks();
    (fs.resolve as any) = path.resolve;
    (fs.join as any) = path.join;
  });

  it('returns empty string if no bootContext in manifest', () => {
    const result = loadBootContext({ ...mockManifest, bootContext: undefined }, workspacePath);
    expect(result).toBe('');
  });

  it('loads files from bootContext.files', () => {
    vi.mocked(fs.existsSync).mockReturnValue(true);
    vi.mocked(fs.readFileSync).mockReturnValue('File content');

    // Mock path.resolve to handle the path traversal check
    const originalResolve = path.resolve;
    vi.spyOn(path, 'resolve').mockImplementation((...args) => {
      const p = originalResolve(...args);
      return p;
    });

    const result = loadBootContext(mockManifest, workspacePath);
    expect(result).toContain('<boot-context label="Test File">');
    expect(result).toContain('File content');
  });

  it('loads files from bootContext.directory', () => {
    const manifestWithDir: RuntimeManifest = {
      ...mockManifest,
      bootContext: {
        directory: 'docs/boot'
      }
    };

    vi.mocked(fs.existsSync).mockReturnValue(true);
    vi.mocked(fs.statSync).mockReturnValue({ isDirectory: () => true } as any);
    vi.mocked(fs.readdirSync).mockReturnValue(['a.md', 'b.txt', 'c.md'] as any);
    vi.mocked(fs.readFileSync).mockImplementation((p: any) => `Content of ${p}`);

    const result = loadBootContext(manifestWithDir, workspacePath);
    expect(result).toContain('<boot-context label="a.md">');
    expect(result).toContain('<boot-context label="c.md">');
    expect(result).not.toContain('b.txt');
  });

  it('respects per-file maxTokens', () => {
    const manifestWithMaxTokens: RuntimeManifest = {
      ...mockManifest,
      bootContext: {
        files: [{ path: 'large.md', label: 'Large File', maxTokens: 5 }]
      }
    };

    vi.mocked(fs.existsSync).mockReturnValue(true);
    vi.mocked(fs.readFileSync).mockReturnValue('This is a very long file content');

    const result = loadBootContext(manifestWithMaxTokens, workspacePath);
    expect(result).toContain('... [truncated]');
  });

  it('respects total budget', () => {
    process.env['BOOT_CONTEXT_BUDGET'] = '5';
    const manifestWithManyFiles: RuntimeManifest = {
      ...mockManifest,
      bootContext: {
        files: [
          { path: 'file1.md', label: 'File 1' },
          { path: 'file2.md', label: 'File 2' },
        ]
      }
    };

    vi.mocked(fs.existsSync).mockReturnValue(true);
    vi.mocked(fs.readFileSync).mockReturnValue('This is a very long file content indeed');

    const result = loadBootContext(manifestWithManyFiles, workspacePath);
    // File 1 will be truncated, File 2 will be skipped
    expect(result).toContain('label="File 1"');
    expect(result).not.toContain('label="File 2"');
    delete process.env['BOOT_CONTEXT_BUDGET'];
  });
});
