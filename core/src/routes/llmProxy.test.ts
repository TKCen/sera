import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Request, Response, NextFunction } from 'express';

// ── Mocks — all vi.mock factories must be self-contained (hoisted) ──────────

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

vi.mock('../lib/config.js', () => ({
  config: {
    llm: { baseUrl: 'http://localhost:1234/v1', apiKey: 'test-key', model: 'test-model' },
    providers: { activeProvider: 'lmstudio', providers: {} },
    getProviderConfig: () => ({ baseUrl: 'http://localhost:1234/v1', apiKey: 'test-key' }),
  },
}));

vi.mock('../lib/providers.js', () => ({
  PROVIDER_CATALOG: [
    {
      id: 'lmstudio',
      name: 'LM Studio',
      models: [{ id: 'test-model', name: 'Test Model' }],
    },
  ],
}));

vi.mock('../lib/llm/ProviderFactory.js', () => ({
  ProviderFactory: {
    createFromModelConfig: vi.fn().mockReturnValue({
      chat: vi.fn().mockResolvedValue({
        content: 'Hello from the LLM!',
        usage: { promptTokens: 10, completionTokens: 5, totalTokens: 15 },
      }),
    }),
  },
}));

import { createLlmProxyRouter } from './llmProxy.js';
import { IdentityService } from '../auth/IdentityService.js';
import { MeteringService } from '../metering/MeteringService.js';
import { createAuthMiddleware } from '../auth/authMiddleware.js';

// ── Helpers ──────────────────────────────────────────────────────────────────

const TEST_SECRET = 'test-secret-for-proxy-tests';

function createTestSetup(budgetOverride?: Partial<{
  allowed: boolean;
  hourlyUsed: number;
  hourlyQuota: number;
  dailyUsed: number;
  dailyQuota: number;
}>) {
  const identityService = new IdentityService(TEST_SECRET);
  const meteringService = new MeteringService();

  vi.spyOn(meteringService, 'checkBudget').mockResolvedValue({
    allowed: true,
    hourlyUsed: 0,
    hourlyQuota: 100000,
    dailyUsed: 0,
    dailyQuota: 1000000,
    ...budgetOverride,
  });
  vi.spyOn(meteringService, 'recordUsage').mockResolvedValue(undefined);

  const router = createLlmProxyRouter(identityService, meteringService);

  const validToken = identityService.signToken({
    agentId: 'test-agent',
    circleId: 'test-circle',
    capabilities: ['internet-access'],
  });

  return { identityService, meteringService, router, validToken };
}

function createMockReqRes(overrides: Record<string, unknown> = {}) {
  const req = {
    headers: {},
    body: {},
    agentIdentity: undefined,
    ...overrides,
  } as unknown as Request;

  const res = {
    status: vi.fn().mockReturnThis(),
    json: vi.fn().mockReturnThis(),
    headersSent: false,
  } as unknown as Response;

  const next: NextFunction = vi.fn();

  return { req, res, next };
}

/**
 * Find the final handler in a router stack for a given method/path.
 */
function getHandler(router: any, method: string, path: string) {
  for (const layer of router.stack) {
    if (layer.route?.path === path && layer.route.methods[method]) {
      // Return all handlers in the route stack (middleware chain + final handler)
      return layer.route.stack.map((s: any) => s.handle);
    }
  }
  return null;
}

/**
 * Execute a sequence of Express handlers (simulates the middleware chain).
 */
async function executeHandlers(handlers: Function[], req: Request, res: Response) {
  for (const handler of handlers) {
    let nextCalled = false;
    const next: NextFunction = () => { nextCalled = true; };
    await handler(req, res, next);
    if (!nextCalled) break; // Handler sent a response or errored
  }
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('LLM Proxy Router', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('Auth Middleware', () => {
    it('should reject requests without Authorization header', () => {
      const { identityService } = createTestSetup();
      const authMiddleware = createAuthMiddleware(identityService);

      const { req, res, next } = createMockReqRes();
      authMiddleware(req, res, next);

      expect(res.status).toHaveBeenCalledWith(401);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ error: expect.stringContaining('Missing') }),
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject requests with invalid token', () => {
      const { identityService } = createTestSetup();
      const authMiddleware = createAuthMiddleware(identityService);

      const { req, res, next } = createMockReqRes({
        headers: { authorization: 'Bearer bad-token' },
      });
      authMiddleware(req, res, next);

      expect(res.status).toHaveBeenCalledWith(401);
      expect(next).not.toHaveBeenCalled();
    });

    it('should accept requests with valid token and populate identity', () => {
      const { identityService, validToken } = createTestSetup();
      const authMiddleware = createAuthMiddleware(identityService);

      const { req, res, next } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
      });
      authMiddleware(req, res, next);

      expect(next).toHaveBeenCalled();
      expect(req.agentIdentity).toBeDefined();
      expect(req.agentIdentity!.agentId).toBe('test-agent');
      expect(req.agentIdentity!.circleId).toBe('test-circle');
    });
  });

  describe('POST /chat/completions', () => {
    it('should proxy a chat request and return OpenAI-format response', async () => {
      const { router, validToken } = createTestSetup();
      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        body: {
          model: 'test-model',
          messages: [{ role: 'user', content: 'Hello' }],
        },
      });

      await executeHandlers(handlers, req, res);

      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          object: 'chat.completion',
          choices: expect.arrayContaining([
            expect.objectContaining({
              message: expect.objectContaining({
                role: 'assistant',
                content: 'Hello from the LLM!',
              }),
            }),
          ]),
          usage: expect.objectContaining({
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
          }),
        }),
      );
    });

    it('should record metering after successful call', async () => {
      const { router, validToken, meteringService } = createTestSetup();
      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        body: {
          messages: [{ role: 'user', content: 'Hello' }],
        },
      });

      await executeHandlers(handlers, req, res);

      // recordUsage is fire-and-forget, give it a tick
      await new Promise(resolve => setTimeout(resolve, 10));

      expect(meteringService.recordUsage).toHaveBeenCalledWith(
        expect.objectContaining({
          agentId: 'test-agent',
          promptTokens: 10,
          completionTokens: 5,
        }),
      );
    });

    it('should reject requests without messages', async () => {
      const { router, validToken } = createTestSetup();
      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        body: {},
      });

      await executeHandlers(handlers, req, res);

      expect(res.status).toHaveBeenCalledWith(400);
    });

    it('should return 429 when budget is exceeded', async () => {
      const { router, validToken } = createTestSetup({
        allowed: false,
        hourlyUsed: 50000,
        hourlyQuota: 10000,
        dailyUsed: 50000,
        dailyQuota: 100000,
      });
      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        body: {
          messages: [{ role: 'user', content: 'Hello' }],
        },
      });

      await executeHandlers(handlers, req, res);

      expect(res.status).toHaveBeenCalledWith(429);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          error: expect.objectContaining({
            type: 'rate_limit_exceeded',
          }),
        }),
      );
    });
  });

  describe('GET /models', () => {
    it('should return the provider catalog as OpenAI model list', async () => {
      const { router, validToken } = createTestSetup();
      const handlers = getHandler(router, 'get', '/models')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
      });

      await executeHandlers(handlers, req, res);

      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          object: 'list',
          data: expect.arrayContaining([
            expect.objectContaining({
              id: 'test-model',
              object: 'model',
              owned_by: 'lmstudio',
            }),
          ]),
        }),
      );
    });
  });
});
