import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ProcessManager } from './ProcessManager.js';
import type { ProcessTask } from './types.js';
import type { AgentResponse } from '../types.js';

// ── Mock Agent ──────────────────────────────────────────────────────────────────

function createMockAgent(roleName: string, response: string) {
  return {
    name: `${roleName}-display`,
    role: roleName,
    process: vi.fn().mockResolvedValue({
      thought: `Thinking about it`,
      finalAnswer: response,
    } satisfies AgentResponse),
    updateLlmProvider: vi.fn(),
  } as any;
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('ProcessManager', () => {
  let manager: ProcessManager;

  beforeEach(() => {
    manager = new ProcessManager();
  });

  describe('sequential', () => {
    it('should execute tasks in order with context chaining', async () => {
      const agentA = createMockAgent('agent-a', 'Result A');
      const agentB = createMockAgent('agent-b', 'Result B');
      const agents = new Map([
        ['agent-a', agentA],
        ['agent-b', agentB],
      ]);

      const tasks: ProcessTask[] = [
        { id: 'task-1', description: 'First task', assignedAgent: 'agent-a' },
        { id: 'task-2', description: 'Second task', assignedAgent: 'agent-b' },
      ];

      const result = await manager.run('sequential', tasks, agents);

      expect(result.processType).toBe('sequential');
      expect(result.results).toHaveLength(2);
      expect(result.results[0]!.status).toBe('completed');
      expect(result.results[0]!.output).toBe('Result A');
      expect(result.results[1]!.status).toBe('completed');
      expect(result.results[1]!.output).toBe('Result B');

      // Second agent should receive context from first
      const secondCall = agentB.process.mock.calls[0]![0] as string;
      expect(secondCall).toContain('Result A');
      expect(secondCall).toContain('Second task');
    });

    it('should handle agent failures gracefully', async () => {
      const agent = createMockAgent('agent-a', '');
      agent.process.mockRejectedValue(new Error('LLM unavailable'));
      const agents = new Map([['agent-a', agent]]);

      const tasks: ProcessTask[] = [
        { id: 'task-1', description: 'Failing task', assignedAgent: 'agent-a' },
      ];

      const result = await manager.run('sequential', tasks, agents);

      expect(result.results[0]!.status).toBe('failed');
      expect(result.results[0]!.error).toContain('LLM unavailable');
    });
  });

  describe('parallel', () => {
    it('should execute all tasks concurrently', async () => {
      const agentA = createMockAgent('agent-a', 'Parallel A');
      const agentB = createMockAgent('agent-b', 'Parallel B');
      const agents = new Map([
        ['agent-a', agentA],
        ['agent-b', agentB],
      ]);

      const tasks: ProcessTask[] = [
        { id: 'task-1', description: 'Task A', assignedAgent: 'agent-a' },
        { id: 'task-2', description: 'Task B', assignedAgent: 'agent-b' },
      ];

      const result = await manager.run('parallel', tasks, agents);

      expect(result.processType).toBe('parallel');
      expect(result.results).toHaveLength(2);
      expect(result.results.every((r) => r.status === 'completed')).toBe(true);
      expect(result.finalOutput).toContain('Parallel A');
      expect(result.finalOutput).toContain('Parallel B');
    });
  });

  describe('hierarchical', () => {
    it('should require a manager agent', async () => {
      const agents = new Map([['worker', createMockAgent('worker', 'work')]]);
      const tasks: ProcessTask[] = [{ id: 't1', description: 'task' }];

      await expect(manager.run('hierarchical', tasks, agents)).rejects.toThrow(/managerAgent/);
    });

    it('should have the manager review worker results', async () => {
      const worker = createMockAgent('worker', 'Worker output');
      const mgr = createMockAgent('manager', 'APPROVED: looks good');

      // Second call: consolidation
      mgr.process
        .mockResolvedValueOnce({ thought: '', finalAnswer: 'APPROVED: good' })
        .mockResolvedValueOnce({ thought: '', finalAnswer: 'Consolidated answer' });

      const agents = new Map([['worker', worker]]);
      const tasks: ProcessTask[] = [
        { id: 't1', description: 'Do the work', assignedAgent: 'worker' },
      ];

      const result = await manager.run('hierarchical', tasks, agents, mgr);

      expect(result.processType).toBe('hierarchical');
      expect(result.results[0]!.status).toBe('completed');
      // Manager should have been called for review + consolidation
      expect(mgr.process).toHaveBeenCalledTimes(2);
    });
  });

  describe('runSingle', () => {
    it('should run a single task with one agent', async () => {
      const agent = createMockAgent('solo', 'Solo answer');
      const result = await manager.runSingle('Do something', agent);

      expect(result.processType).toBe('sequential');
      expect(result.results).toHaveLength(1);
      expect(result.finalOutput).toBe('Solo answer');
    });
  });

  describe('unknown type', () => {
    it('should throw for unknown process type', async () => {
      await expect(manager.run('unknown' as any, [], new Map())).rejects.toThrow(
        /Unknown process type/
      );
    });
  });
});
