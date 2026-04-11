import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import path from 'node:path';
import type { Request, Response, NextFunction } from 'express';

// ── Platform-aware test paths ────────────────────────────────────────────────
// path.resolve('/granted') on Windows = 'D:\granted', on Linux = '/granted'
const GRANTED_DIR = path.resolve('/granted');
const GRANTED_FILE = path.resolve('/granted/file.txt');
const GRANTED_DELETE = path.resolve('/granted/todelete.txt');
const GRANTED_PERSISTED = path.resolve('/granted/persisted.txt');
const SOME_PATH_FILE = path.resolve('/some/path/file.txt');

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockFiles = new Map<string, string>();
const mockDirs = new Set<string>([GRANTED_DIR, path.resolve('/granted/subdir')]);

vi.mock('node:fs', () => ({
  default: {
    existsSync: vi.fn((p: string) => mockFiles.has(p) || mockDirs.has(p)),
    statSync: vi.fn((p: string) => ({
      isDirectory: () => mockDirs.has(p),
      isFile: () => mockFiles.has(p),
      size: mockFiles.get(p)?.length ?? 0,
    })),
    readFileSync: vi.fn((p: string) => {
      const content = mockFiles.get(p);
      if (content === undefined) throw new Error('ENOENT');
      return content;
    }),
    writeFileSync: vi.fn((_p: string, _content: string) => {}),
    mkdirSync: vi.fn(),
    readdirSync: vi.fn((p: string) => {
      if (p === GRANTED_DIR) {
        return [
          { name: 'file.txt', isDirectory: () => false, isFile: () => true },
          { name: 'subdir', isDirectory: () => true, isFile: () => false },
        ];
      }
      return [];
    }),
    realpathSync: vi.fn((p: string) => p),
    unlinkSync: vi.fn(),
    rmSync: vi.fn(),
  },
}));

// Mock authMiddleware to inject agent identity
vi.mock('../auth/authMiddleware.js', () => ({
  createAuthMiddleware: vi.fn(() => (req: Request, _res: Response, next: NextFunction) => {
    req.agentIdentity = {
      agentId: 'agent-1',
      agentName: 'test-agent',
      circleId: 'default',
      capabilities: [],
      scope: 'agent' as const,
      iat: 0,
      exp: 0,
    };
    next();
  }),
}));

// Mock PermissionRequestService
const mockHasActiveGrant = vi.fn();
const mockPermissionService = {
  hasActiveGrant: mockHasActiveGrant,
} as unknown as import('../sandbox/PermissionRequestService.js').PermissionRequestService;

// Mock AgentRegistry
const mockGetActiveFilesystemGrants = vi.fn();
const mockRegistry = {
  getActiveFilesystemGrants: mockGetActiveFilesystemGrants,
} as unknown as import('../agents/registry.service.js').AgentRegistry;

const mockIdentityService = {} as import('../auth/IdentityService.js').IdentityService;
const mockAuthService = {} as import('../auth/auth-service.js').AuthService;

// ── Test Suite ────────────────────────────────────────────────────────────────

describe('Tool Proxy Route (POST /v1/tools/proxy)', () => {
  let app: express.Express;

  beforeAll(async () => {
    const { createToolProxyRouter } = await import('./toolProxy.js');

    app = express();
    app.use(express.json());
    app.use(
      '/v1/tools',
      createToolProxyRouter(
        mockIdentityService,
        mockAuthService,
        mockPermissionService,
        mockRegistry
      )
    );
  });

  beforeEach(() => {
    mockHasActiveGrant.mockReset();
    mockGetActiveFilesystemGrants.mockReset();
    mockFiles.clear();
  });

  it('returns 400 for invalid tool name', async () => {
    const res = await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'shell-exec', args: { command: 'ls' } });

    expect(res.status).toBe(400);
    expect(res.body.error).toContain('Invalid tool');
  });

  it('returns 400 for missing path arg', async () => {
    const res = await request(app).post('/v1/tools/proxy').send({ tool: 'file-read', args: {} });

    expect(res.status).toBe(400);
    expect(res.body.error).toContain('Missing required arg: path');
  });

  it('returns 403 when no grant covers the path', async () => {
    mockHasActiveGrant.mockReturnValue(false);
    mockGetActiveFilesystemGrants.mockResolvedValue([]);

    const res = await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'file-read', args: { path: '/unauthorized/file.txt' } });

    expect(res.status).toBe(403);
    expect(res.body.error).toBe('grant_not_found');
  });

  it('allows file-read when session grant covers the path', async () => {
    mockHasActiveGrant.mockReturnValue(true);
    mockFiles.set(GRANTED_FILE, 'hello world');

    const res = await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'file-read', args: { path: '/granted/file.txt' } });

    expect(res.status).toBe(200);
    expect(res.body.result).toBe('hello world');
  });

  it('allows file-write when session grant covers the path', async () => {
    mockHasActiveGrant.mockReturnValue(true);

    const res = await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'file-write', args: { path: '/granted/new.txt', content: 'new content' } });

    expect(res.status).toBe(200);
    expect(res.body.result).toContain('File written');
  });

  it('returns 400 for file-write without content', async () => {
    mockHasActiveGrant.mockReturnValue(true);

    const res = await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'file-write', args: { path: '/granted/new.txt' } });

    expect(res.status).toBe(400);
    expect(res.body.error).toContain('Missing required arg: content');
  });

  it('allows file-list when session grant covers the path', async () => {
    mockHasActiveGrant.mockReturnValue(true);

    const res = await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'file-list', args: { path: '/granted' } });

    expect(res.status).toBe(200);
    expect(res.body.result).toBeInstanceOf(Array);
  });

  it('allows file-delete when session grant covers the path', async () => {
    mockHasActiveGrant.mockReturnValue(true);
    mockFiles.set(GRANTED_DELETE, 'bye');

    const res = await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'file-delete', args: { path: '/granted/todelete.txt' } });

    expect(res.status).toBe(200);
    expect(res.body.result).toContain('Deleted file');
  });

  it('falls back to persistent grants when no session grant exists', async () => {
    mockHasActiveGrant.mockReturnValue(false);
    mockGetActiveFilesystemGrants.mockResolvedValue([
      { id: 'grant-1', value: GRANTED_DIR, grant_type: 'persistent' },
    ]);
    mockFiles.set(GRANTED_PERSISTED, 'persistent content');

    const res = await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'file-read', args: { path: '/granted/persisted.txt' } });

    expect(res.status).toBe(200);
    expect(res.body.result).toBe('persistent content');
  });

  it('validates grant via hasActiveGrant with the canonicalised path', async () => {
    mockHasActiveGrant.mockReturnValue(false);
    mockGetActiveFilesystemGrants.mockResolvedValue([]);

    await request(app)
      .post('/v1/tools/proxy')
      .send({ tool: 'file-read', args: { path: '/some/path/file.txt' } });

    // Path is canonicalised by path.resolve, so it'll be platform-specific
    expect(mockHasActiveGrant).toHaveBeenCalledWith('agent-1', 'filesystem', SOME_PATH_FILE);
  });
});
