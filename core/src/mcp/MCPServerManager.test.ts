import { describe, it, expect, vi, beforeEach } from 'vitest';
import { MCPServerManager, type MCPServerManifest } from './MCPServerManager.js';
import { SandboxManager } from '../sandbox/SandboxManager.js';

// Mock SandboxManager
vi.mock('../sandbox/SandboxManager.js', () => {
  class Mock {
    spawn = vi.fn().mockResolvedValue({
      containerId: 'mcp-container-123',
      status: 'running',
      chatUrl: 'http://localhost:3000/sse',
    });
    teardown = vi.fn().mockResolvedValue(undefined);
  }
  return { SandboxManager: Mock };
});

// Mock Logger
vi.mock('../lib/logger.js', () => {
  return {
    Logger: class {
      info = vi.fn();
      error = vi.fn();
      warn = vi.fn();
      debug = vi.fn();
    },
  };
});

describe('MCPServerManager', () => {
  let manager: MCPServerManager;
  let sandboxManager: SandboxManager;

  const mockManifest: MCPServerManifest = {
    apiVersion: 'sera/v1',
    kind: 'SkillProvider',
    metadata: {
      name: 'test-mcp',
      description: 'Test MCP server',
    },
    image: 'mcp-image',
    transport: 'stdio',
    command: 'node',
    args: ['index.js'],
  };

  beforeEach(() => {
    sandboxManager = new SandboxManager({} as any);
    manager = new MCPServerManager(sandboxManager);
    vi.clearAllMocks();
  });

  describe('spawnServer', () => {
    it('should spawn a server with stdio transport', async () => {
      const result = await manager.spawnServer(mockManifest);

      expect(sandboxManager.spawn).toHaveBeenCalled();
      expect(result.clientOptions.transport).toBe('stdio');
      expect(result.clientOptions.command).toBe('docker');
      expect(result.clientOptions.args).toEqual([
        'exec', '-i', 'mcp-container-123', 'node', 'index.js'
      ]);
    });

    it('should spawn a server with http transport', async () => {
      const httpManifest: MCPServerManifest = {
        ...mockManifest,
        transport: 'http',
        url: 'http://example.com:8080/mcp',
      };

      const result = await manager.spawnServer(httpManifest);

      expect(result.clientOptions.transport).toBe('http');
      expect(result.clientOptions.url).toBe('http://mcp-containe:8080/mcp');
    });

    it('should use default command and args for stdio if not provided', async () => {
      const minimalManifest: MCPServerManifest = {
        apiVersion: 'sera/v1',
        kind: 'SkillProvider',
        metadata: { name: 'minimal' },
        image: 'minimal-image',
        transport: 'stdio',
      };

      const result = await manager.spawnServer(minimalManifest);

      expect(result.clientOptions.args).toEqual([
        'exec', '-i', 'mcp-container-123', 'npm', 'start'
      ]);
    });
  });

  describe('stopServer', () => {
    it('should call sandboxManager.teardown', async () => {
      await manager.stopServer('inst-123');
      expect(sandboxManager.teardown).toHaveBeenCalledWith('inst-123');
    });
  });
});
