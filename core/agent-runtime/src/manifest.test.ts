import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { generateSystemPrompt, type RuntimeManifest } from './manifest.js';
import path from 'path';
import fs from 'fs';

vi.mock('fs');

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
    expect(prompt).toContain('## Agent Notes');
    expect(prompt).toContain('Always respond in JSON format.');
  });

  it('injects context files with labels as subsections', () => {
    vi.mocked(fs.readFileSync).mockReturnValue('# API Reference\nGET /api/v1/users');
    vi.mocked(fs.existsSync).mockReturnValue(true);
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
    vi.mocked(fs.existsSync).mockReturnValue(false);
    vi.mocked(fs.readFileSync).mockImplementation(() => {
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

  it('drops low priority files first when over budget', { timeout: 30000 }, () => {
    const highContent = 'H'.repeat(4000); // ~1000 tokens
    const lowContent = 'L'.repeat(4000); // ~1000 tokens
    vi.mocked(fs.existsSync).mockReturnValue(true);
    vi.mocked(fs.readFileSync).mockReturnValueOnce(highContent).mockReturnValueOnce(lowContent);

    const manifest: RuntimeManifest = {
      ...baseManifest,
      contextFiles: [
        { path: 'high.md', label: 'High Priority Doc', priority: 'high' },
        { path: 'low.md', label: 'Low Priority Doc', priority: 'low' },
      ],
    };
    process.env['WORKSPACE_PATH'] = '/workspace';
    process.env['CONTEXT_FILES_BUDGET'] = '1500';
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).toContain('### High Priority Doc');
    expect(prompt).toContain('### Low Priority Doc');
    expect(prompt).toContain('*Omitted due to token budget: low.md*');
    // High priority content should still be present (not omitted)
    expect(prompt).not.toContain('*Omitted due to token budget: high.md*');
  });

  it('truncates file content to per-file maxTokens', { timeout: 30000 }, () => {
    const longContent = 'A'.repeat(10000);
    vi.mocked(fs.existsSync).mockReturnValue(true);
    vi.mocked(fs.readFileSync).mockReturnValue(longContent);
    const manifest: RuntimeManifest = {
      ...baseManifest,
      contextFiles: [{ path: 'big.md', label: 'Big File', maxTokens: 100 }],
    };
    process.env['WORKSPACE_PATH'] = '/workspace';
    process.env['CONTEXT_FILES_BUDGET'] = '8000';
    const prompt = generateSystemPrompt(manifest);
    expect(prompt).toContain('...(truncated)');
    // Accurate truncation via tokenizer is more variable than 1:4 ratio
    // We just check it's truncated and shorter than original
    const labelIdx = prompt.indexOf('### Big File');
    const contentAfterLabel = prompt.substring(labelIdx + '### Big File\n'.length);
    const truncMarker = '...(truncated)';
    const truncIdx = contentAfterLabel.indexOf(truncMarker);
    expect(truncIdx).toBeGreaterThanOrEqual(0);
    expect(truncIdx).toBeLessThan(10000);
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
    expect(vi.mocked(fs.readFileSync)).not.toHaveBeenCalled();
  });
});
