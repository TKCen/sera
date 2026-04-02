import { describe, it, expect, vi } from 'vitest';
import type { Request } from 'express';
import { AuthService } from './auth-service.js';
import type { AuthPlugin, OperatorIdentity } from './interfaces.js';

describe('AuthService', () => {
  it('should register a plugin and use it for authentication', async () => {
    const authService = new AuthService();
    const mockIdentity: OperatorIdentity = {
      sub: 'user-1',
      roles: ['admin'],
      authMethod: 'api-key',
    };
    const mockPlugin: AuthPlugin = {
      name: 'test-plugin',
      authenticate: vi.fn().mockResolvedValue(mockIdentity),
    };

    authService.registerPlugin(mockPlugin);
    const result = await authService.authenticate({} as Request);
    expect(result).toEqual(mockIdentity);
    expect(mockPlugin.authenticate).toHaveBeenCalled();
  });

  it('should return null if no plugins are registered', async () => {
    const authService = new AuthService();
    const mockReq = {} as Request;
    const result = await authService.authenticate(mockReq);
    expect(result).toBeNull();
  });

  it('should return identity if a plugin authenticates successfully', async () => {
    const authService = new AuthService();
    const mockIdentity: OperatorIdentity = {
      sub: 'user-123',
      roles: ['admin'],
      authMethod: 'api-key',
    };
    const mockPlugin: AuthPlugin = {
      name: 'success-plugin',
      authenticate: vi.fn().mockResolvedValue(mockIdentity),
    };

    authService.registerPlugin(mockPlugin);
    const mockReq = {} as Request;
    const result = await authService.authenticate(mockReq);

    expect(result).toEqual(mockIdentity);
    expect(mockPlugin.authenticate).toHaveBeenCalledWith(mockReq);
  });

  it('should try multiple plugins until one succeeds', async () => {
    const authService = new AuthService();
    const mockIdentity: OperatorIdentity = {
      sub: 'user-456',
      roles: ['operator'],
      authMethod: 'oidc',
    };

    const plugin1: AuthPlugin = {
      name: 'fail-1',
      authenticate: vi.fn().mockResolvedValue(null),
    };
    const plugin2: AuthPlugin = {
      name: 'success-2',
      authenticate: vi.fn().mockResolvedValue(mockIdentity),
    };
    const plugin3: AuthPlugin = {
      name: 'unused-3',
      authenticate: vi.fn(),
    };

    authService.registerPlugin(plugin1);
    authService.registerPlugin(plugin2);
    authService.registerPlugin(plugin3);

    const mockReq = {} as Request;
    const result = await authService.authenticate(mockReq);

    expect(result).toEqual(mockIdentity);
    expect(plugin1.authenticate).toHaveBeenCalled();
    expect(plugin2.authenticate).toHaveBeenCalled();
    expect(plugin3.authenticate).not.toHaveBeenCalled();
  });

  it('should return null if all plugins return null', async () => {
    const authService = new AuthService();
    const plugin1: AuthPlugin = {
      name: 'fail-1',
      authenticate: vi.fn().mockResolvedValue(null),
    };
    const plugin2: AuthPlugin = {
      name: 'fail-2',
      authenticate: vi.fn().mockResolvedValue(null),
    };

    authService.registerPlugin(plugin1);
    authService.registerPlugin(plugin2);

    const mockReq = {} as Request;
    const result = await authService.authenticate(mockReq);

    expect(result).toBeNull();
  });

  it('should propagate errors from plugins', async () => {
    const authService = new AuthService();
    const plugin: AuthPlugin = {
      name: 'error-plugin',
      authenticate: vi.fn().mockRejectedValue(new Error('Invalid token')),
    };

    authService.registerPlugin(plugin);

    const mockReq = {} as Request;
    await expect(authService.authenticate(mockReq)).rejects.toThrow('Invalid token');
  });
});
