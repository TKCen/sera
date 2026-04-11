import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ProcessManager } from './ProcessManager.js';
import type { ProcessTask } from './types.js';
import type { AgentResponse } from '../types.js';
import type { BaseAgent } from '../BaseAgent.js';

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
  } as unknown as BaseAgent;
}

// ── Tests ────────────────────────────────────────────────────────────────────────

describe('FlowProcess', () => {
  let manager: ProcessManager;

  beforeEach(() => {
    manager = new ProcessManager();
  });

  it('should execute tasks sequentially with dependency resolution', async () => {
    const agentA = createMockAgent('agent-a', 'Output A');
    const agentB = createMockAgent('agent-b', 'Output B');
    const agents = new Map([
      ['agent-a', agentA],
      ['agent-b', agentB],
    ]);

    const tasks: ProcessTask[] = [
      { id: 't1', description: 'Task 1', assignedAgent: 'agent-a' },
      { id: 't2', description: 'Task 2', assignedAgent: 'agent-b', dependsOn: ['t1'] },
    ];

    const result = await manager.run('flow', tasks, agents);

    expect(result.processType).toBe('flow');
    expect(result.results).toHaveLength(2);
    expect(result.results[0]!.taskId).toBe('t1');
    expect(result.results[1]!.taskId).toBe('t2');

    // Check context passing
    const bCall = vi.mocked(agentB.process).mock.calls[0]![0] as string;
    expect(bCall).toContain('[Task t1]: Output A');
  });

  it('should handle "and" routing (wait for all)', async () => {
    const agentA = createMockAgent('agent-a', 'A');
    const agentB = createMockAgent('agent-b', 'B');
    const agentC = createMockAgent('agent-c', 'C');
    const agents = new Map([
      ['agent-a', agentA],
      ['agent-b', agentB],
      ['agent-c', agentC],
    ]);

    const tasks: ProcessTask[] = [
      { id: 't1', description: 'Task 1', assignedAgent: 'agent-a' },
      { id: 't2', description: 'Task 2', assignedAgent: 'agent-b' },
      {
        id: 't3',
        description: 'Task 3',
        assignedAgent: 'agent-c',
        dependsOn: ['t1', 't2'],
        routingType: 'and',
      },
    ];

    const result = await manager.run('flow', tasks, agents);

    expect(result.results).toHaveLength(3);
    const cCall = vi.mocked(agentC.process).mock.calls[0]![0] as string;
    expect(cCall).toContain('[Task t1]: A');
    expect(cCall).toContain('[Task t2]: B');
  });

  it('should handle "or" routing (wait for any)', async () => {
    const agentA = createMockAgent('agent-a', 'A');
    const agentB = createMockAgent('agent-b', 'B');
    const agents = new Map([
      ['agent-a', agentA],
      ['agent-b', agentB],
    ]);

    // t2 depends on t1 but we'll run it as "or" (effectively same as sequential here since t1 finishes first)
    const tasks: ProcessTask[] = [
      { id: 't1', description: 'Task 1', assignedAgent: 'agent-a' },
      {
        id: 't2',
        description: 'Task 2',
        assignedAgent: 'agent-b',
        dependsOn: ['t1'],
        routingType: 'or',
      },
    ];

    const result = await manager.run('flow', tasks, agents);
    expect(result.results).toHaveLength(2);
  });

  it('should respect conditions', async () => {
    const agentA = createMockAgent('agent-a', 'This is a test');
    const agents = new Map([['agent-a', agentA]]);

    const tasks: ProcessTask[] = [
      { id: 't1', description: 'Task 1', assignedAgent: 'agent-a', condition: 'SUCCESS' },
    ];

    const result = await manager.run('flow', tasks, agents);
    expect(result.results[0]!.status).toBe('failed');
    expect(result.results[0]!.error).toBe('Condition not met');
  });
});
