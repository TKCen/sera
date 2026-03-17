import { describe, it, expect, vi } from 'vitest';
import { shellExecSkill } from './shell-exec.js';
import type { AgentContext } from '../types.js';
import path from 'path';

describe('shellExecSkill', () => {
  const mockContext: AgentContext = {
    agentName: 'TestAgent',
    workspacePath: process.cwd(),
    tier: 2,
    agentInstanceId: 'test-instance',
    containerId: undefined,
    sandboxManager: undefined,
  };

  it('should successfully execute a basic shell command', async () => {
    const params = { command: 'echo "hello"' };
    const result = await shellExecSkill.handler(params, mockContext);

    expect(result.success).toBe(true);
    expect(result.data).toContain('hello');
  });

  it('should return error if tier is less than 2', async () => {
    const params = { command: 'echo "hello"' };
    const lowTierContext = { ...mockContext, tier: 1 };
    const result = await shellExecSkill.handler(params, lowTierContext);

    expect(result.success).toBe(false);
    expect(result.error).toContain('Agent tier must be 2 or higher');
  });

  it('should return error if command fails', async () => {
    const params = { command: 'this_command_does_not_exist_12345' };
    const result = await shellExecSkill.handler(params, mockContext);

    expect(result.success).toBe(false);
    expect(result.error).toContain('this_command_does_not_exist_12345');
  });

  it('should execute command in the workspace directory', async () => {
    const params = { command: 'node -e "console.log(process.cwd())"' };
    const workspacePath = path.resolve(process.cwd());
    const result = await shellExecSkill.handler(params, mockContext);

    expect(result.success).toBe(true);
    // Check if the output contains the workspace path
    // Using trim() to remove newline character from the output
    // Note: depending on the OS, path.resolve might look slightly different,
    // so we check if the path ends with the expected directory or contains it.
    const outputString = String(result.data).trim();
    // Normalize paths for comparison (e.g. windows vs unix separators)
    const normalizedOutput = outputString.replace(/\\/g, '/');
    const normalizedWorkspace = workspacePath.replace(/\\/g, '/');
    expect(normalizedOutput).toContain(normalizedWorkspace);
  });
});
