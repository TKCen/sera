import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { generateSystemPrompt, type RuntimeManifest } from './manifest.js';
import path from 'path';

// Mock fs to avoid real file system access
vi.mock('fs', async () => {
  const actual = await vi.importActual<typeof import('fs')>('fs');
  return {
    ...actual,
    default: {
      ...actual,
      readFileSync: vi.fn(),
      existsSync: vi.fn(),
    },
  };
});

import fs from 'fs';

const baseManifest: RuntimeManifest = {
  apiVersion: 'sera/v1',
  kind: 'Agent',
  metadata: {
    name: 'test-agent',
    displayName: 'Test Agent',
    icon: '🤖',
    circle: 'default',
    tier: 1,
  },
  identity: {
    role: 'Test role',
    description: 'A test agent',
  },
  model: {
    provider: 'openai',
    name: 'gpt-4o',
  },
};

describe('generateSystemPrompt', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    delete process.env['WORKSPACE_PATH'];
    delete process.env['CONTEXT_FILES_BUDGET'];
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it('produces a baseline prompt without notes or contextFiles', () => {
    const prompt = generateSystemPrompt(baseManifest);
    expect(prompt).toContain('You are Test Agent');
    expect(prompt).toContain('Role: Test role');
    expect(prompt).not.toContain('## Notes');
    expect(prompt).not.toContain('## Workspace Context');
  });

  it('injects notes section when manifest.notes is set', () => {
    const manifest: RuntimeManifest = {
      ...baseManifest,
      notes: 'Always respond in JSON format.',
    };
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).toContain('## Notes');
    expect(prompt).toContain('Always respond in JSON format.');
  });

  it('injects context files with labels as subsections', () => {
    (fs.readFileSync as ReturnType<typeof vi.fn>).mockReturnValue(
      '# API Reference\nGET /api/v1/users'
    );
    const manifest: RuntimeManifest = {
      ...baseManifest,
      contextFiles: [{ path: 'docs/api.md', label: 'API Docs' }],
    };
    process.env['WORKSPACE_PATH'] = '/workspace';
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).toContain('## Workspace Context');
    expect(prompt).toContain('### API Docs');
    expect(prompt).toContain('# API Reference');
    expect(fs.readFileSync).toHaveBeenCalledWith(path.join('/workspace', 'docs/api.md'), 'utf-8');
  });

  it('produces "*File not found*" warning instead of throwing for missing files', () => {
    (fs.readFileSync as ReturnType<typeof vi.fn>).mockImplementation(() => {
      throw new Error('ENOENT: no such file or directory');
    });
    const manifest: RuntimeManifest = {
      ...baseManifest,
      contextFiles: [{ path: 'missing/file.md', label: 'Missing Doc' }],
    };
    process.env['WORKSPACE_PATH'] = '/workspace';
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).toContain('## Workspace Context');
    expect(prompt).toContain('### Missing Doc');
    expect(prompt).toContain('*File not found: missing/file.md*');
  });

  it('drops low priority files first when over budget', () => {
    const highContent = 'H'.repeat(8000 * 4); // ~8000 tokens
    const lowContent = 'L'.repeat(2000 * 4); // ~2000 tokens
    (fs.readFileSync as ReturnType<typeof vi.fn>)
      .mockReturnValueOnce(highContent)
      .mockReturnValueOnce(lowContent);

    const manifest: RuntimeManifest = {
      ...baseManifest,
      contextFiles: [
        { path: 'high.md', label: 'High Priority Doc', priority: 'high' },
        { path: 'low.md', label: 'Low Priority Doc', priority: 'low' },
      ],
    };
    process.env['WORKSPACE_PATH'] = '/workspace';
    process.env['CONTEXT_FILES_BUDGET'] = '8000';
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).toContain('### High Priority Doc');
    expect(prompt).toContain('### Low Priority Doc');
    expect(prompt).toContain('*Omitted due to token budget: low.md*');
    // High priority content should still be present (not omitted)
    expect(prompt).not.toContain('*Omitted due to token budget: high.md*');
  });

  it('truncates file content to per-file maxTokens', () => {
    const longContent = 'A'.repeat(10000);
    (fs.readFileSync as ReturnType<typeof vi.fn>).mockReturnValue(longContent);
    const manifest: RuntimeManifest = {
      ...baseManifest,
      contextFiles: [{ path: 'big.md', label: 'Big File', maxTokens: 100 }],
    };
    process.env['WORKSPACE_PATH'] = '/workspace';
    process.env['CONTEXT_FILES_BUDGET'] = '8000';
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).toContain('...(truncated)');
    // maxTokens=100 means maxChars=400; content should be 400 chars + truncation marker
    const labelIdx = prompt.indexOf('### Big File');
    const contentAfterLabel = prompt.substring(labelIdx + '### Big File\n'.length);
    const truncMarker = '...(truncated)';
    const truncIdx = contentAfterLabel.indexOf(truncMarker);
    expect(truncIdx).toBeGreaterThanOrEqual(0);
    // Content before truncation marker should be at most 400 chars
    expect(truncIdx).toBeLessThanOrEqual(400 + 1); // +1 for newline before marker
  });

  it('does not add Workspace Context section for empty contextFiles array', () => {
    const manifest: RuntimeManifest = {
      ...baseManifest,
      contextFiles: [],
    };
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).not.toContain('## Workspace Context');
  });

  it('blocks path traversal attempts', () => {
    const manifest: RuntimeManifest = {
      ...baseManifest,
      contextFiles: [{ path: '../../etc/passwd', label: 'Secrets' }],
    };
    process.env['WORKSPACE_PATH'] = '/workspace';
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).toContain('*Path traversal blocked: ../../etc/passwd*');
    expect(fs.readFileSync).not.toHaveBeenCalled();
  });
});
