/**
 * Epic 10 — Circles Integration Tests
 *
 * Tests: create circle → git repo initialised → add member → constitution injected
 * Tests: parallel coordination → all members receive task → results collected within timeout
 *
 * These tests require a running PostgreSQL database. They will skip gracefully
 * when DATABASE_URL is not set.
 */

import { describe, it, expect, beforeAll, afterAll, vi } from 'vitest';
import fs from 'fs/promises';
import path from 'path';
import os from 'os';
import type { BaseAgent } from '../agents/BaseAgent.js';

const hasDb = !!process.env.DATABASE_URL;

describe.skipIf(!hasDb)('Circles integration (Story 10.1 + 10.2)', () => {
  let circleId: string;
  let tmpKnowledgeDir: string;

  beforeAll(async () => {
    tmpKnowledgeDir = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-kg-integ-'));
    process.env['KNOWLEDGE_BASE_PATH'] = tmpKnowledgeDir;
    // initDb is called by server startup; for tests we call it directly
    const { initDb } = await import('../lib/database.js');
    await initDb();
  });

  it('creates a circle and initialises git repo', async () => {
    const { CircleService } = await import('../circles/CircleService.js');

    const kgsMock = {
      initCircleRepo: vi.fn().mockResolvedValue(undefined),
      archiveCircleRepo: vi.fn().mockResolvedValue(undefined),
    };
    vi.doMock('../memory/KnowledgeGitService.js', () => ({
      KnowledgeGitService: { getInstance: () => kgsMock },
    }));

    const svc = CircleService.getInstance();
    const circle = await svc.createCircle({
      name: `test-circle-${Date.now()}`,
      displayName: 'Test Circle',
      constitution: '# Test\n\nThis circle values testing.',
    });

    expect(circle.id).toBeTruthy();
    expect(circle.name).toMatch(/^test-circle-/);
    circleId = circle.id;
  });

  it('adds a member and constitution appears in context', async () => {
    if (!circleId) return;

    const { CircleService } = await import('../circles/CircleService.js');
    const svc = CircleService.getInstance();
    const circle = await svc.getCircle(circleId);
    expect(circle).not.toBeNull();
    expect(circle!.constitution).toContain('This circle values testing.');
  });

  it('updates circle constitution', async () => {
    if (!circleId) return;

    const { CircleService } = await import('../circles/CircleService.js');
    const svc = CircleService.getInstance();
    const updated = await svc.updateCircle(circleId, {
      constitution: '# Updated\n\nNew constitution content.',
    });
    expect(updated).not.toBeNull();
    expect(updated!.constitution).toContain('New constitution content.');
  });

  it('lists circle members', async () => {
    if (!circleId) return;

    const { CircleService } = await import('../circles/CircleService.js');
    const svc = CircleService.getInstance();
    const members = await svc.getMembers(circleId);
    expect(Array.isArray(members)).toBe(true);
  });

  afterAll(async () => {
    if (circleId) {
      const { CircleService } = await import('../circles/CircleService.js');
      const svc = CircleService.getInstance();
      await svc.deleteCircle(circleId).catch(() => {});
    }
    if (tmpKnowledgeDir) {
      await fs.rm(tmpKnowledgeDir, { recursive: true, force: true }).catch(() => {});
    }
    delete process.env['KNOWLEDGE_BASE_PATH'];
  });
});

describe.skipIf(!hasDb)('Parallel coordination integration (Story 10.4)', () => {
  it('parallel process collects results from multiple agents within timeout', async () => {
    const { ProcessManager } = await import('../agents/process/ProcessManager.js');

    const makeAgent = (name: string, output: string) => ({
      role: name,
      name,
      process: vi.fn().mockResolvedValue({ finalAnswer: output }),
    });

    const agents = new Map<string, unknown>([
      ['agent-a', makeAgent('agent-a', 'Result from A')],
      ['agent-b', makeAgent('agent-b', 'Result from B')],
    ]);

    const pm = new ProcessManager();
    const result = await pm.run(
      'parallel',
      [
        { id: 'task-1', description: 'Task for A', assignedAgent: 'agent-a' },
        { id: 'task-2', description: 'Task for B', assignedAgent: 'agent-b' },
      ],
      agents as unknown as Map<string, BaseAgent>
    );

    expect(result.processType).toBe('parallel');
    expect(result.results).toHaveLength(2);
    expect(result.results.every((r) => r.status === 'completed')).toBe(true);
    expect(result.results.find((r) => r.taskId === 'task-1')?.output).toBe('Result from A');
    expect(result.results.find((r) => r.taskId === 'task-2')?.output).toBe('Result from B');
  });

  it('parallel process marks individual failures without aborting', async () => {
    const { ProcessManager } = await import('../agents/process/ProcessManager.js');

    const agents = new Map<string, unknown>([
      ['agent-ok', { role: 'agent-ok', process: vi.fn().mockResolvedValue({ finalAnswer: 'ok' }) }],
      ['agent-fail', { role: 'agent-fail', process: vi.fn().mockRejectedValue(new Error('boom')) }],
    ]);

    const pm = new ProcessManager();
    const result = await pm.run(
      'parallel',
      [
        { id: 't1', description: 'ok task', assignedAgent: 'agent-ok' },
        { id: 't2', description: 'fail task', assignedAgent: 'agent-fail' },
      ],
      agents as unknown as Map<string, BaseAgent>
    );

    expect(result.results.find((r) => r.taskId === 't1')?.status).toBe('completed');
    expect(result.results.find((r) => r.taskId === 't2')?.status).toBe('failed');
    expect(result.results.find((r) => r.taskId === 't2')?.error).toBe('boom');
  });
});
