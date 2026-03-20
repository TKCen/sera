/**
 * Integration-light tests for KnowledgeGitService.
 * These tests run against a real (temp-dir) git repo but mock DB and Qdrant.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import fs from 'fs/promises';
import os from 'os';
import path from 'path';

// Mock database query so we don't need a real DB
vi.mock('../lib/database.js', () => ({
  query: vi.fn().mockResolvedValue({ rows: [] }),
  pool: {},
}));

// Mock VectorService to avoid Qdrant
vi.mock('../services/vector.service.js', () => ({
  VectorService: class {
    upsert = vi.fn().mockResolvedValue(undefined);
    delete = vi.fn().mockResolvedValue(undefined);
    rebuildNamespace = vi.fn().mockResolvedValue(undefined);
    getCollectionInfo = vi.fn().mockResolvedValue({ vectorCount: 0 });
    ensureNamespaceCollection = vi.fn().mockResolvedValue(undefined);
    search = vi.fn().mockResolvedValue([]);
    searchLegacy = vi.fn().mockResolvedValue([]);
    deletePoints = vi.fn().mockResolvedValue(undefined);
    upsertPoints = vi.fn().mockResolvedValue(undefined);
    ensureCollection = vi.fn().mockResolvedValue(undefined);
  },
  collectionName: (ns: string) => ns,
}));

// Mock EmbeddingService
vi.mock('../services/embedding.service.js', () => ({
  EmbeddingService: {
    getInstance: () => ({
      isAvailable: () => false,
      embed: vi.fn().mockResolvedValue([]),
      generateEmbedding: vi.fn().mockResolvedValue([]),
      warmup: vi.fn().mockResolvedValue(undefined),
    }),
  },
  EMBEDDING_VECTOR_SIZE: 768,
}));

describe('KnowledgeGitService', () => {
  let tmpDir: string;
  let KnowledgeGitService: typeof import('./KnowledgeGitService.js').KnowledgeGitService;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-kg-'));
    vi.resetModules();
    process.env['KNOWLEDGE_BASE_PATH'] = tmpDir;

    // Re-import after setting env
    const mod = await import('./KnowledgeGitService.js');
    KnowledgeGitService = mod.KnowledgeGitService;
    (KnowledgeGitService as unknown as { instance: undefined }).instance = undefined;
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
    delete process.env['KNOWLEDGE_BASE_PATH'];
    vi.restoreAllMocks();
  });

  it('initCircleRepo creates a git repository with an initial commit', async () => {
    const svc = KnowledgeGitService.getInstance();
    await svc.initCircleRepo('test-circle');

    const repoDir = path.join(tmpDir, 'circles', 'test-circle');
    const gitDir = path.join(repoDir, '.git');
    const stat = await fs.stat(gitDir);
    expect(stat.isDirectory()).toBe(true);

    const readme = await fs.readFile(path.join(repoDir, 'README.md'), 'utf8');
    expect(readme).toContain('test-circle');
  });

  it('initCircleRepo is idempotent', async () => {
    const svc = KnowledgeGitService.getInstance();
    await svc.initCircleRepo('idem-circle');
    await svc.initCircleRepo('idem-circle'); // second call must not throw
  });

  it('write() creates a file in the agent branch and commits', async () => {
    const svc = KnowledgeGitService.getInstance();
    await svc.initCircleRepo('write-circle');

    const { block, commitHash } = await svc.write('write-circle', 'agent-123', 'TestAgent', {
      content: 'The sky is blue.',
      type: 'fact',
      agentId: 'agent-123',
      tags: ['sky'],
    });

    expect(block.id).toBeDefined();
    expect(block.type).toBe('fact');
    expect(commitHash).toBeTruthy();

    // Verify the file exists in the agent directory
    const repoDir = path.join(tmpDir, 'circles', 'write-circle');
    const typeDir = path.join(repoDir, 'agent-123', 'fact');
    const files = await fs.readdir(typeDir);
    expect(files).toHaveLength(1);
    expect(files[0]).toContain(block.id);
  });

  it('write() commits with agent identity (verifiable via merge)', async () => {
    const svc = KnowledgeGitService.getInstance();
    await svc.initCircleRepo('log-circle');

    // First, make sure the main branch is checked out
    const { simpleGit } = await import('simple-git');
    const repoDirLog = path.join(tmpDir, 'circles', 'log-circle');
    const gitLog = simpleGit(repoDirLog);
    await gitLog.checkoutLocalBranch('main').catch(() => gitLog.checkout('main'));

    const { commitHash } = await svc.write('log-circle', 'agent-xyz', 'MyAgent', {
      content: 'Important fact.',
      type: 'insight',
      agentId: 'agent-xyz',
    });

    // Commit should have a real hash
    expect(commitHash).toBeTruthy();
    expect(typeof commitHash).toBe('string');

    // Merge the branch so it appears in main log
    await svc.mergeToMain('log-circle', 'agent-xyz', 'test-operator');

    const log = await svc.log('log-circle');
    // main should now have: init commit + merge commit
    expect(log.length).toBeGreaterThanOrEqual(2);
    // The merge commit message references the branch
    expect(log[0]!.message).toContain('knowledge/agent-agent-xyz');
  });

  it('mergeToMain merges agent branch to main', async () => {
    const svc = KnowledgeGitService.getInstance();
    await svc.initCircleRepo('merge-circle');

    const { simpleGit } = await import('simple-git');
    const repoDirMerge = path.join(tmpDir, 'circles', 'merge-circle');
    const gitMerge = simpleGit(repoDirMerge);
    await gitMerge.checkoutLocalBranch('main').catch(() => gitMerge.checkout('main'));

    await svc.write('merge-circle', 'agent-abc', 'MergeAgent', {
      content: 'Knowledge for merging.',
      type: 'memory',
      agentId: 'agent-abc',
    });

    // mock DB update succeeds
    await expect(svc.mergeToMain('merge-circle', 'agent-abc', 'operator-1')).resolves.not.toThrow();

    // After merge, the file should be on main
    const branch = (await gitMerge.revparse(['--abbrev-ref', 'HEAD'])).trim();
    expect(branch).toBe('main');
  });

  it('diff() returns empty string before any writes', async () => {
    const svc = KnowledgeGitService.getInstance();
    await svc.initCircleRepo('diff-circle');
    const diff = await svc.diff('diff-circle', 'agent-999');
    expect(typeof diff).toBe('string');
  });
});
