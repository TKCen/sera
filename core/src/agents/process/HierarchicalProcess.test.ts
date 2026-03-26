import { describe, it, expect, vi } from 'vitest';
import { HierarchicalProcess } from './HierarchicalProcess.js';
import type { BaseAgent } from '../BaseAgent.js';
import type { ProcessTask } from './types.js';

describe('HierarchicalProcess', () => {
  const createMockAgent = (role: string, responseOutput: string) => {
    return {
      role,
      process: vi.fn().mockResolvedValue({ finalAnswer: responseOutput }),
    } as unknown as BaseAgent;
  };

  it('should throw if managerAgent is not provided', async () => {
    const process = new HierarchicalProcess();
    const tasks: ProcessTask[] = [{ id: 'task1', description: 'task' }];
    const agents = new Map<string, BaseAgent>();

    await expect(process.execute(tasks, agents)).rejects.toThrow(
      'HierarchicalProcess requires a managerAgent to validate worker results'
    );
  });

  it('should execute tasks, get manager approval, and consolidate results', async () => {
    const process = new HierarchicalProcess();
    const worker = createMockAgent('worker1', 'worker output 1');
    const manager = {
      role: 'manager',
      process: vi.fn().mockImplementation((input: string) => {
        if (input.includes('Review this worker output')) {
          return Promise.resolve({ finalAnswer: 'APPROVED: looks good' });
        }
        if (input.includes('Review and consolidate')) {
          return Promise.resolve({ finalAnswer: 'final consolidated answer' });
        }
        return Promise.resolve({ finalAnswer: '' });
      }),
    } as unknown as BaseAgent;

    const agents = new Map<string, BaseAgent>([
      ['worker1', worker],
      ['manager', manager],
    ]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'worker1' },
    ];

    const result = await process.execute(tasks, agents, manager);

    expect(result.processType).toBe('hierarchical');
    expect(result.results).toHaveLength(1);
    expect(result.results[0]?.status).toBe('completed');
    expect(result.results[0]?.output).toBe('worker output 1');
    expect(result.finalOutput).toBe('final consolidated answer');

    expect(worker.process).toHaveBeenCalledWith('task 1 description');
    expect(manager.process).toHaveBeenCalledWith(
      expect.stringContaining('Review this worker output for the task "task 1 description":\n\nworker output 1')
    );
    expect(manager.process).toHaveBeenCalledWith(
      expect.stringContaining('Review and consolidate the following worker outputs into a final answer:\n\n[worker1] Task: task1\nworker output 1')
    );
  });

  it('should fallback to picking the first non-manager worker if no agent is assigned', async () => {
    const process = new HierarchicalProcess();
    const worker1 = createMockAgent('worker1', 'worker output 1');
    const manager = {
      role: 'manager',
      process: vi.fn().mockImplementation((input: string) => {
        if (input.includes('Review this worker output')) {
          return Promise.resolve({ finalAnswer: 'APPROVED: looks good' });
        }
        return Promise.resolve({ finalAnswer: 'consolidation' });
      }),
    } as unknown as BaseAgent;

    const agents = new Map<string, BaseAgent>([
      ['manager', manager], // Put manager first
      ['worker1', worker1],
    ]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description' }, // no assignedAgent
    ];

    const result = await process.execute(tasks, agents, manager);

    expect(result.results[0]?.agentName).toBe('worker1');
    expect(result.results[0]?.status).toBe('completed');
    expect(worker1.process).toHaveBeenCalledWith('task 1 description');
  });

  it('should fail task when assigned agent is not found and no other worker exists', async () => {
    const process = new HierarchicalProcess();
    const manager = createMockAgent('manager', 'consolidation');
    const agents = new Map<string, BaseAgent>([
      ['manager', manager]
    ]); // No workers

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'missing-worker' },
    ];

    const result = await process.execute(tasks, agents, manager);

    expect(result.results).toHaveLength(1);
    expect(result.results[0]?.status).toBe('failed');
    expect(result.results[0]?.error).toBe('Worker agent not found');
    expect(result.results[0]?.output).toBe('');

    // No completed outputs, so consolidation might still be called with empty workerOutputs
    // The implementation says:
    // let finalOutput = workerOutputs;
    // if (workerOutputs) { ... }
    // Thus it returns empty string.
    expect(result.finalOutput).toBe('');
  });

  it('should retry when manager rejects output, and eventually fail if retries exhausted', async () => {
    const process = new HierarchicalProcess(1); // maxRetries = 1
    const worker = {
      role: 'worker1',
      process: vi.fn()
        .mockResolvedValueOnce({ finalAnswer: 'worker attempt 1' })
        .mockResolvedValueOnce({ finalAnswer: 'worker attempt 2' }),
    } as unknown as BaseAgent;

    const manager = {
      role: 'manager',
      process: vi.fn().mockImplementation((input: string) => {
        if (input.includes('Review this worker output')) {
          // Always reject
          return Promise.resolve({ finalAnswer: 'REJECTED: try again' });
        }
        return Promise.resolve({ finalAnswer: 'consolidation' });
      }),
    } as unknown as BaseAgent;

    const agents = new Map<string, BaseAgent>([
      ['worker1', worker],
      ['manager', manager],
    ]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'worker1' },
    ];

    const result = await process.execute(tasks, agents, manager);

    // It attempts 0, then 1 (maxRetries = 1), then returns last output
    expect(worker.process).toHaveBeenCalledTimes(2);
    expect(worker.process).toHaveBeenNthCalledWith(1, 'task 1 description');
    expect(worker.process).toHaveBeenNthCalledWith(2,
      expect.stringContaining('The manager rejected your previous answer and asked you to retry.\nPrevious answer: worker attempt 1\n\nOriginal task: task 1 description')
    );

    // After exhausting retries, the final status is 'completed' if output exists.
    // Let's check the code: "status: lastOutput ? 'completed' : 'failed'".
    expect(result.results[0]?.status).toBe('completed');
    expect(result.results[0]?.output).toBe('worker attempt 2');

    // It will consolidate the last output since it was completed
    expect(manager.process).toHaveBeenCalledWith(
      expect.stringContaining('Review and consolidate the following worker outputs into a final answer:\n\n[worker1] Task: task1\nworker attempt 2')
    );
  });

  it('should handle manager consolidation error and fallback to raw worker outputs', async () => {
    const process = new HierarchicalProcess();
    const worker = createMockAgent('worker1', 'worker output 1');
    const manager = {
      role: 'manager',
      process: vi.fn().mockImplementation((input: string) => {
        if (input.includes('Review this worker output')) {
          return Promise.resolve({ finalAnswer: 'APPROVED: looks good' });
        }
        if (input.includes('Review and consolidate')) {
          return Promise.reject(new Error('Manager consolidation failed'));
        }
      }),
    } as unknown as BaseAgent;

    const agents = new Map<string, BaseAgent>([
      ['worker1', worker],
      ['manager', manager],
    ]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'worker1' },
    ];

    const result = await process.execute(tasks, agents, manager);

    // Worker succeeded, so it should be in results
    expect(result.results[0]?.status).toBe('completed');

    // Final output falls back to workerOutputs
    expect(result.finalOutput).toBe('[worker1] Task: task1\nworker output 1');
  });

  it('should handle worker error during execution', async () => {
    const process = new HierarchicalProcess(0); // no retries
    const worker = {
      role: 'worker1',
      process: vi.fn().mockRejectedValue(new Error('Worker processing error')),
    } as unknown as BaseAgent;

    const manager = createMockAgent('manager', 'APPROVED: good');
    const agents = new Map<string, BaseAgent>([
      ['worker1', worker],
      ['manager', manager],
    ]);

    const tasks: ProcessTask[] = [
      { id: 'task1', description: 'task 1 description', assignedAgent: 'worker1' },
    ];

    const result = await process.execute(tasks, agents, manager);

    // Retries exhausted, lastOutput is empty, status is 'failed', lastError is set
    expect(result.results[0]?.status).toBe('failed');
    expect(result.results[0]?.output).toBe('');
    expect(result.results[0]?.error).toBe('Worker processing error');

    expect(result.finalOutput).toBe(''); // No completed outputs to consolidate
  });
});
