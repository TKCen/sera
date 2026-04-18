import { describe, it, expect, vi } from 'vitest';
import { ParallelProcess } from './ParallelProcess.js';
import type { BaseAgent } from '../BaseAgent.js';
import type { ProcessTask } from './types.js';

describe('ParallelProcess', () => {
  const createMockAgent = (role: string, responseOutput: string) => {
    return {
      role,
      process: vi.fn().mockResolvedValue({ finalAnswer: responseOutput }),
    } as unknown as BaseAgent;
  };

  it('should execute tasks concurrently and aggregate outputs', async () => {
    const process = new ParallelProcess();
    const agent1 = createMockAgent('agent1', 'output1');
    const agent2 = createMockAgent('agent2', 'output2');

    const agents = new Map<string, BaseAgent>([
      ['agent1', agent1],
      ['agent2', agent2],
    ]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'agent1' },
      { id: 'task2', description: 'task 2 description', assignedAgent: 'agent2' },
    ];

    const result = await process.execute(tasks, agents);

    expect(result.processType).toBe('parallel');
    expect(result.results).toHaveLength(2);
    expect(result.results[0]?.status).toBe('completed');
    expect(result.results[0]?.output).toBe('output1');
    expect(result.results[1]?.status).toBe('completed');
    expect(result.results[1]?.output).toBe('output2');

    // finalOutput should be an aggregation of completed outputs
    expect(result.finalOutput).toBe('[agent1]: output1\n\n[agent2]: output2');

    expect(agent1.process).toHaveBeenCalledWith('task 1 description');
    expect(agent2.process).toHaveBeenCalledWith('task 2 description');
  });

  it('should fallback to first available agent if no agent is assigned', async () => {
    const process = new ParallelProcess();
    const agent1 = createMockAgent('agent1', 'output1');
    const agents = new Map<string, BaseAgent>([['agent1', agent1]]);

    const tasks: ProcessTask[] = [{ id: 'task1', description: 'task 1 description' }];

    const result = await process.execute(tasks, agents);

    expect(result.results).toHaveLength(1);
    expect(result.results[0]?.agentName).toBe('agent1');
    expect(result.results[0]?.status).toBe('completed');
    expect(result.results[0]?.output).toBe('output1');
    expect(result.finalOutput).toBe('[agent1]: output1');
    expect(agent1.process).toHaveBeenCalledWith('task 1 description');
  });

  it('should fail task when assigned agent is not found', async () => {
    const process = new ParallelProcess();
    const agents = new Map<string, BaseAgent>(); // Empty map

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'missing-agent' },
    ];

    const result = await process.execute(tasks, agents);

    expect(result.results).toHaveLength(1);
    expect(result.results[0]?.status).toBe('failed');
    expect(result.results[0]?.error).toBe('Agent "missing-agent" not found');
    expect(result.results[0]?.output).toBe('');
    expect(result.finalOutput).toBe('');
  });

  it('should handle agent processing errors', async () => {
    const process = new ParallelProcess();
    const agent1 = {
      role: 'agent1',
      process: vi.fn().mockRejectedValue(new Error('Processing error')),
    } as unknown as BaseAgent;
    const agent2 = createMockAgent('agent2', 'output2');

    const agents = new Map<string, BaseAgent>([
      ['agent1', agent1],
      ['agent2', agent2],
    ]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'agent1' },
      { id: 'task2', description: 'task 2 description', assignedAgent: 'agent2' },
    ];

    const result = await process.execute(tasks, agents);

    expect(result.results).toHaveLength(2);

    // task1 fails
    expect(result.results[0]?.status).toBe('failed');
    expect(result.results[0]?.error).toBe('Processing error');

    // task2 completes
    expect(result.results[1]?.status).toBe('completed');
    expect(result.results[1]?.output).toBe('output2');

    // finalOutput should only contain completed outputs
    expect(result.finalOutput).toBe('[agent2]: output2');
  });
});
