import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Request, Response, NextFunction } from 'express';

// ── Mocks ─────────────────────────────────────────────────────────────────────

vi.mock('../lib/database.js', () => ({
  query: vi.fn(),
}));

vi.mock('../lib/config.js', () => ({
  config: {
    llm: { baseUrl: 'http://localhost:4000/v1', apiKey: 'test-key', model: 'test-model' },
    providers: { activeProvider: 'lmstudio', providers: {} },
    getProviderConfig: () => ({ baseUrl: 'http://localhost:4000/v1', apiKey: 'test-key' }),
  },
}));

// Mock LlmRouter — default returns a successful response
vi.mock('../llm/LlmRouter.js', () => ({
  LlmRouter: vi.fn().mockImplementation(() => ({
    chatCompletion: vi.fn().mockResolvedValue({
      response: {
        id: 'chatcmpl-test',
        object: 'chat.completion',
        created: 1234567890,
        model: 'test-model',
        choices: [
          {
            index: 0,
            message: { role: 'assistant', content: 'Hello from the LLM!' },
            finish_reason: 'stop',
          },
        ],
        usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
      },
      latencyMs: 100,
    }),
    chatCompletionStream: vi.fn(),
    listModels: vi
      .fn()
      .mockResolvedValue([{ id: 'test-model', object: 'model', owned_by: 'lmstudio' }]),
    addModel: vi.fn(),
    deleteModel: vi.fn(),
    testModel: vi.fn(),
  })),
  // ProviderRegistry mock (imported by LlmRouter consumers)
  ProviderRegistry: vi.fn().mockImplementation(() => ({})),
}));

// Mock the CircuitBreakerService — default passes through to client.chatCompletion
vi.mock('../llm/CircuitBreakerService.js', () => ({
  CircuitBreakerService: vi.fn().mockImplementation((client: any) => ({
    client,
    call: vi
      .fn()
      .mockImplementation((req: any, agentId: any, latencyStart: any) =>
        client.chatCompletion(req, agentId, latencyStart)
      ),
    getState: vi.fn().mockReturnValue([]),
    getProviderState: vi.fn().mockReturnValue(null),
  })),
  providerFromModel: vi.fn().mockReturnValue('lmstudio'),
}));

vi.mock('../middleware/rateLimitStub.js', () => ({
  rateLimitStub: vi.fn().mockImplementation((_req: any, _res: any, next: any) => next()),
}));

import { createLlmProxyRouter } from './llmProxy.js';
import { IdentityService } from '../auth/IdentityService.js';
import { AuthService } from '../auth/auth-service.js';
import { MeteringService } from '../metering/MeteringService.js';
import { LlmRouter } from '../llm/LlmRouter.js';
import { CircuitBreakerService } from '../llm/CircuitBreakerService.js';
import { createAuthMiddleware } from '../auth/authMiddleware.js';

// ── Helpers ───────────────────────────────────────────────────────────────────

const TEST_SECRET = 'test-secret-for-proxy-tests';

async function createTestSetup(
  budgetOverride?: Partial<{
    allowed: boolean;
    hourlyUsed: number;
    hourlyQuota: number;
    dailyUsed: number;
    dailyQuota: number;
  }>
) {
  const identityService = new IdentityService(TEST_SECRET);
  const meteringService = new MeteringService();
  const llmRouter = new LlmRouter({} as any);
  const circuitBreakerService = new CircuitBreakerService(llmRouter);

  vi.spyOn(meteringService, 'checkBudget').mockResolvedValue({
    allowed: true,
    hourlyUsed: 0,
    hourlyQuota: 100000,
    dailyUsed: 0,
    dailyQuota: 1000000,
    ...budgetOverride,
  });
  vi.spyOn(meteringService, 'recordUsage').mockResolvedValue(undefined);

  const authService = new AuthService();
  const pool = {} as any;
  const orchestrator = { getManifest: vi.fn(), getManifestByInstanceId: vi.fn() } as any;
  const router = createLlmProxyRouter(
    identityService,
    authService,
    meteringService,
    llmRouter,
    circuitBreakerService,
    pool,
    orchestrator
  );

  const validToken = await identityService.signToken({
    agentId: 'test-agent',
    circleId: 'test-circle',
    capabilities: ['internet-access'],
    scope: 'agent',
  });

  return { identityService, meteringService, llmRouter, circuitBreakerService, router, validToken };
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
    setHeader: vi.fn(),
    end: vi.fn(),
    headersSent: false,
  } as unknown as Response;

  const next: NextFunction = vi.fn();

  return { req, res, next };
}

function getHandler(router: any, method: string, path: string) {
  for (const layer of router.stack) {
    if (layer.route?.path === path && layer.route.methods[method]) {
      return layer.route.stack.map((s: any) => s.handle);
    }
  }
  return null;
}

async function executeHandlers(handlers: Function[], req: Request, res: Response) {
  for (const handler of handlers) {
    let nextCalled = false;
    const next: NextFunction = () => {
      nextCalled = true;
    };
    await handler(req, res, next);
    if (!nextCalled) break;
  }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('LLM Proxy Router', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('Auth Middleware', () => {
    it('should reject requests without Authorization header', async () => {
      const { identityService } = await createTestSetup();
      const authService = new AuthService();
      const authMiddleware = createAuthMiddleware(identityService, authService);

      const { req, res, next } = createMockReqRes();
      await authMiddleware(req, res, next);

      expect(res.status).toHaveBeenCalledWith(401);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ error: expect.stringContaining('Missing') })
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('should reject requests with invalid token', async () => {
      const { identityService } = await createTestSetup();
      const authService = new AuthService();
      const authMiddleware = createAuthMiddleware(identityService, authService);

      const { req, res, next } = createMockReqRes({
        headers: { authorization: 'Bearer bad-token' },
      });
      await authMiddleware(req, res, next);

      expect(res.status).toHaveBeenCalledWith(401);
      expect(next).not.toHaveBeenCalled();
    });

    it('should accept requests with valid token and populate identity', async () => {
      const { identityService, validToken } = await createTestSetup();
      const authService = new AuthService();
      const authMiddleware = createAuthMiddleware(identityService, authService);

      const { req, res, next } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
      });
      await authMiddleware(req, res, next);

      expect(next).toHaveBeenCalled();
      expect(req.agentIdentity).toBeDefined();
      expect(req.agentIdentity!.agentId).toBe('test-agent');
      expect(req.agentIdentity!.circleId).toBe('test-circle');
      expect(req.agentIdentity!.scope).toBe('agent');
    });
  });

  describe('POST /chat/completions', () => {
    it('should proxy a chat request and return OpenAI-format response', async () => {
      const { router, validToken } = await createTestSetup();
      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        agentIdentity: {
          agentId: 'test-agent',
          circleId: 'test-circle',
          scope: 'agent',
          capabilities: [],
          agentName: 'test-agent',
          iat: 0,
          exp: 9999999999,
        },
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
        })
      );
    });

    it('should record metering after successful call', async () => {
      const { router, validToken, meteringService } = await createTestSetup();
      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        agentIdentity: {
          agentId: 'test-agent',
          circleId: 'test-circle',
          scope: 'agent',
          capabilities: [],
          agentName: 'test-agent',
          iat: 0,
          exp: 9999999999,
        },
        body: {
          messages: [{ role: 'user', content: 'Hello' }],
        },
      });

      await executeHandlers(handlers, req, res);

      // recordUsage is fire-and-forget, give it a tick
      await new Promise((resolve) => setTimeout(resolve, 10));

      expect(meteringService.recordUsage).toHaveBeenCalledWith(
        expect.objectContaining({
          agentId: 'test-agent',
          promptTokens: 10,
          completionTokens: 5,
          totalTokens: 15,
          status: 'success',
        })
      );
    });

    it('should reject requests without messages', async () => {
      const { router, validToken } = await createTestSetup();
      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        body: {},
      });

      await executeHandlers(handlers, req, res);

      expect(res.status).toHaveBeenCalledWith(400);
    });

    it('should return 429 with budget_exceeded when budget is exceeded', async () => {
      const { router, validToken } = await createTestSetup({
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
          error: 'budget_exceeded',
          period: expect.stringMatching(/hourly|daily/),
        })
      );
    });

    it('INVARIANT: no upstream call is made when budget is exceeded', async () => {
      // Story 4.3: budget must be checked BEFORE the upstream call — a failed
      // budget check must never result in an upstream call being placed.
      const { router, llmRouter, validToken } = await createTestSetup({
        allowed: false,
        hourlyUsed: 99999,
        hourlyQuota: 100,
        dailyUsed: 99999,
        dailyQuota: 100,
      });
      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        body: { messages: [{ role: 'user', content: 'Hello' }] },
      });

      await executeHandlers(handlers, req, res);

      // Budget exceeded — upstream LiteLLM must NOT have been called
      expect(res.status).toHaveBeenCalledWith(429);
      expect((llmRouter as any).chatCompletion).not.toHaveBeenCalled();
    });

    it('should return 503 when circuit breaker is open', async () => {
      const { router, circuitBreakerService, validToken } = await createTestSetup();
      // Make circuit breaker throw CIRCUIT_OPEN error
      const err = new Error('Provider lmstudio is currently unavailable (circuit open)');
      (err as any).code = 'CIRCUIT_OPEN';
      (err as any).provider = 'lmstudio';
      vi.spyOn(circuitBreakerService, 'call').mockRejectedValue(err);

      const handlers = getHandler(router, 'post', '/chat/completions')!;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        body: { messages: [{ role: 'user', content: 'Hello' }] },
      });

      await executeHandlers(handlers, req, res);

      expect(res.status).toHaveBeenCalledWith(503);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ error: 'provider_unavailable', provider: 'lmstudio' })
      );
    });
  });

  describe('GET /models', () => {
    it('should return model list from LiteLLM', async () => {
      const { router, validToken } = await createTestSetup();
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
        })
      );
    });
  });
});
