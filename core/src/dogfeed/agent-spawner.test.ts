import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AgentSpawner } from './agent-spawner.js';
import type { DogfeedTask } from './types.js';
import { createDefaultConfig } from './constants.js';

describe('AgentSpawner', () => {
  let spawner: AgentSpawner;

  beforeEach(() => {
    const config = createDefaultConfig({ repoRoot: '/tmp/test-repo' });
    // Pass a mock docker instance to avoid connecting to real Docker
    spawner = new AgentSpawner(config, {} as never);
  });

  describe('routeTask', () => {
    it('routes lint tasks to pi-agent', () => {
      const task: DogfeedTask = {
        priority: 1,
        category: 'lint',
        description: 'Remove unused import',
        status: 'ready',
      };
      expect(spawner.routeTask(task)).toBe('pi-agent');
    });

    it('routes todo tasks to pi-agent', () => {
      const task: DogfeedTask = {
        priority: 1,
        category: 'todo',
        description: 'Remove dead TODO comment',
        status: 'ready',
      };
      expect(spawner.routeTask(task)).toBe('pi-agent');
    });

    it('routes dead-code tasks to pi-agent', () => {
      const task: DogfeedTask = {
        priority: 2,
        category: 'dead-code',
        description: 'Remove unused function',
        status: 'ready',
      };
      expect(spawner.routeTask(task)).toBe('pi-agent');
    });

    it('routes test tasks to omc', () => {
      const task: DogfeedTask = {
        priority: 1,
        category: 'test',
        description: 'Add missing test for service',
        status: 'ready',
      };
      expect(spawner.routeTask(task)).toBe('omc');
    });

    it('routes refactor tasks to omc', () => {
      const task: DogfeedTask = {
        priority: 2,
        category: 'refactor',
        description: 'Extract duplicate error handling',
        status: 'ready',
      };
      expect(spawner.routeTask(task)).toBe('omc');
    });

    it('routes infra tasks to omc', () => {
      const task: DogfeedTask = {
        priority: 0,
        category: 'infra',
        description: 'Build dogfeed loop',
        status: 'ready',
      };
      expect(spawner.routeTask(task)).toBe('omc');
    });

    it('routes feature tasks to omc', () => {
      const task: DogfeedTask = {
        priority: 1,
        category: 'feature',
        description: 'Add new endpoint',
        status: 'ready',
      };
      expect(spawner.routeTask(task)).toBe('omc');
    });

    it('routes type-error tasks to omc', () => {
      const task: DogfeedTask = {
        priority: 1,
        category: 'type-error',
        description: 'Fix type error in service',
        status: 'ready',
      };
      expect(spawner.routeTask(task)).toBe('omc');
    });
  });
});
