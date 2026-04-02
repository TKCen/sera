import { describe, it, expect, vi } from 'vitest';
import { fileWriteSkill } from './file-write.js';
import type { AgentContext } from '../types.js';
import type { SecurityTier } from '../../agents/manifest/types.js';
import path from 'path';

describe('fileWriteSkill security', () => {
  const mockSandboxManager = {
    exec: vi.fn().mockResolvedValue({ exitCode: 0, output: '' }),
  };

  const mockContext: AgentContext = {
    agentName: 'TestAgent',
    workspacePath: '/tmp/sera-test',
    tier: 1 as SecurityTier,
    manifest: {
      apiVersion: 'v1',
      kind: 'Agent',
      metadata: {
        name: 'TestAgent',
        displayName: 'Test Agent',
        icon: '',
        circle: 'test',
        tier: 1 as SecurityTier,
      },
      identity: { role: 'tester', description: 'Test agent' },
      model: { provider: 'openai', name: 'gpt-4' },
    },
    agentInstanceId: 'test-instance',
    containerId: 'test-container',
    sandboxManager: mockSandboxManager as unknown as import('../../sandbox/SandboxManager.js').SandboxManager,
    sessionId: 'test-session',
  };

  it('should be vulnerable to command injection in path', async () => {
    const maliciousPath = 'test.txt"; touch /tmp/pwned; echo "';
    const params = {
      path: maliciousPath,
      content: 'hello',
    };

    await fileWriteSkill.handler(params, mockContext);

    expect(mockSandboxManager.exec).toHaveBeenCalled();
    const execArgs = mockSandboxManager.exec.mock.calls[0]![1];
    const command = execArgs.command;

    // The command is ['sh', '-c', script, '--', ...]
    const script = command[2];

    // The script should NOT contain the malicious path anymore, it should use positional parameters.
    expect(script).not.toContain(maliciousPath);
    expect(script).toContain('"$1"');
    expect(script).toContain('"$2"');
    expect(script).toContain('"$3"');

    // Check that the malicious path is passed as a separate argument
    expect(command).toContain(path.posix.join('/workspace', maliciousPath));
  });
});
