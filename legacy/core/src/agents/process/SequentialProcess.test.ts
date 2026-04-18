import { describe, it, expect, vi } from 'vitest';
import { SequentialProcess } from './SequentialProcess.js';
import type { BaseAgent } from '../BaseAgent.js';
import type { ProcessTask } from './types.js';

describe('SequentialProcess', () => {
  const createMockAgent = (role: string, responseOutput: string) => {
    return {
      role,
      process: vi.fn().mockResolvedValue({ finalAnswer: responseOutput }),
    } as unknown as BaseAgent;
  };

  it('should execute tasks sequentially, passing previous output to next task', async () => {
    const process = new SequentialProcess();
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

    expect(result.processType).toBe('sequential');
    expect(result.results).toHaveLength(2);
    expect(result.results[0]?.output).toBe('output1');
    expect(result.results[0]?.status).toBe('completed');
    expect(result.results[1]?.output).toBe('output2');
    expect(result.results[1]?.status).toBe('completed');
    expect(result.finalOutput).toBe('output2');

    expect(agent1.process).toHaveBeenCalledWith('task 1 description');
    expect(agent2.process).toHaveBeenCalledWith(
      'Previous context:\noutput1\n\nCurrent task:\ntask 2 description'
    );
  });

  it('should fallback to first available agent if no agent is assigned', async () => {
    const process = new SequentialProcess();
    const agent1 = createMockAgent('agent1', 'output1');

    const agents = new Map<string, BaseAgent>([['agent1', agent1]]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description' }, // no assignedAgent
    ];

    const result = await process.execute(tasks, agents);

    expect(result.results[0]?.agentName).toBe('agent1');
    expect(result.results[0]?.status).toBe('completed');
    expect(agent1.process).toHaveBeenCalledWith('task 1 description');
  });

  it('should handle task with unassigned agent when agent map is empty or assigned agent is missing', async () => {
    const process = new SequentialProcess();
    const agent1 = createMockAgent('agent1', 'output1');
    const agents = new Map<string, BaseAgent>([['agent1', agent1]]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'missing-agent' },
      { id: 'task2', description: 'task 2 description', assignedAgent: 'agent1' },
    ];

    const result = await process.execute(tasks, agents);

    expect(result.results).toHaveLength(2);
    expect(result.results[0]?.status).toBe('failed');
    expect(result.results[0]?.error).toBe('Agent "missing-agent" not found');

    // Because task 1 failed to even find an agent, its output is empty.
    // Wait, let's look at the implementation:
    // "continue" skips the loop iteration, so previousOutput remains ''.
    expect(result.results[1]?.status).toBe('completed');
    expect(result.results[1]?.output).toBe('output1');
    // And agent 1 receives just task 2 description since previousOutput was '' initially
    expect(agent1.process).toHaveBeenCalledWith('task 2 description');
  });

  it('should handle agent processing errors and continue with empty context', async () => {
    const process = new SequentialProcess();
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
    expect(result.results[0]?.status).toBe('failed');
    expect(result.results[0]?.error).toBe('Processing error');

    expect(result.results[1]?.status).toBe('completed');
    expect(result.results[1]?.output).toBe('output2');

    // Since task1 threw, previousOutput should be empty for task2
    expect(agent2.process).toHaveBeenCalledWith('task 2 description');
  });
});
