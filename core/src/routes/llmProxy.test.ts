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

interface LlmRouterMockInstance {
  chatCompletion: import('vitest').Mock;
  chatCompletionStream: import('vitest').Mock;
  listModels: import('vitest').Mock;
  addModel: import('vitest').Mock;
  deleteModel: import('vitest').Mock;
  testModel: import('vitest').Mock;
}

const { mockLlmRouter, mockProviderRegistry } = vi.hoisted(() => ({
  mockLlmRouter: vi.fn().mockImplementation(function (this: LlmRouterMockInstance) {
    this.chatCompletion = vi.fn().mockResolvedValue({
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
    });
    this.chatCompletionStream = vi.fn();
    this.listModels = vi
      .fn()
      .mockResolvedValue([{ id: 'test-model', object: 'model', owned_by: 'lmstudio' }]);
    this.addModel = vi.fn();
    this.deleteModel = vi.fn();
    this.testModel = vi.fn();
  }),
  mockProviderRegistry: vi.fn().mockImplementation(() => ({})),
}));

vi.mock('../llm/LlmRouter.js', () => ({
  LlmRouter: mockLlmRouter,
  ProviderRegistry: mockProviderRegistry,
}));

// Mock the CircuitBreakerService — default passes through to client.chatCompletion
vi.mock('../llm/CircuitBreakerService.js', () => {
  interface CircuitBreakerServiceMockInstance {
    client: unknown;
    call: import('vitest').Mock;
    getState: import('vitest').Mock;
    getProviderState: import('vitest').Mock;
  }

  const CircuitBreakerServiceMock = vi.fn().mockImplementation(function (
    this: CircuitBreakerServiceMockInstance,
    client: unknown
  ) {
    this.client = client;
    this.call = vi
      .fn()
      .mockImplementation((req: unknown, agentId: unknown, latencyStart: unknown) =>
        (
          client as {
            chatCompletion: (req: unknown, agentId: unknown, latencyStart: unknown) => unknown;
          }
        ).chatCompletion(req, agentId, latencyStart)
      );
    this.getState = vi.fn().mockReturnValue([]);
    this.getProviderState = vi.fn().mockReturnValue(null);
  });

  return {
    CircuitBreakerService: CircuitBreakerServiceMock,
    providerFromModel: vi.fn().mockReturnValue('lmstudio'),
  };
});

vi.mock('../middleware/rateLimitStub.js', () => ({
  rateLimitStub: vi
    .fn()
    .mockImplementation((_req: unknown, _res: unknown, next: () => void) => next()),
}));

import { ContextAssembler } from '../llm/ContextAssembler.js';
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
  const llmRouter = {
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
  } as unknown as LlmRouter;
  const circuitBreakerService = new CircuitBreakerService(
    llmRouter as unknown as import('../llm/CircuitBreakerService.js').CircuitBreakerService['client']
  );

  vi.spyOn(meteringService, 'checkBudget').mockResolvedValue({
    allowed: true,
    hourlyUsed: 0,
    hourlyQuota: 100000,
    dailyUsed: 0,
    dailyQuota: 1000000,
    ...budgetOverride,
  });
  vi.spyOn(meteringService, 'recordUsage').mockResolvedValue(undefined);

  // Mock ContextAssembler to simulate memory injection
  vi.spyOn(ContextAssembler.prototype, 'assemble').mockImplementation(
    async (agentId, messages, onEvent) => {
      onEvent?.({
        stage: 'memory.retrieved',
        detail: {
          blocks: [
            {
              id: 'block-1',
              source: 'personal:agent-1',
              relevance: 0.95,
              content: 'Some knowledge content',
            },
          ],
        },
      });
      return messages;
    }
  );

  const authService = new AuthService();
  const pool = {} as unknown as import('pg').Pool;
  const orchestrator = {
    getManifest: vi.fn(),
    getManifestByInstanceId: vi.fn(),
  } as unknown as import('../agents/Orchestrator.js').Orchestrator;
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

function getHandler(router: unknown, method: string, path: string) {
  for (const layer of (
    router as unknown as {
      stack: {
        route?: { path: string; methods: Record<string, boolean>; stack: { handle: unknown }[] };
      }[];
    }
  ).stack) {
    if (layer.route?.path === path && layer.route.methods[method]) {
      return layer.route.stack.map((s: { handle: unknown }) => s.handle);
    }
  }
  return null;
}

async function executeHandlers(
  handlers: ((
    req: Request,
    res: Response,
    next: import('express').NextFunction
  ) => void | Promise<void>)[],
  req: Request,
  res: Response
) {
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
      const handlers = getHandler(router, 'post', '/chat/completions')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

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
      const handlers = getHandler(router, 'post', '/chat/completions')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

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
      const handlers = getHandler(router, 'post', '/chat/completions')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

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
      const handlers = getHandler(router, 'post', '/chat/completions')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

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
      const handlers = getHandler(router, 'post', '/chat/completions')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        body: { messages: [{ role: 'user', content: 'Hello' }] },
      });

      await executeHandlers(handlers, req, res);

      // Budget exceeded — upstream LiteLLM must NOT have been called
      expect(res.status).toHaveBeenCalledWith(429);
      expect(
        (llmRouter as unknown as { chatCompletion: import('vitest').Mock }).chatCompletion
      ).not.toHaveBeenCalled();
    });

    it('should include citations in response when agent uses injected memory', async () => {
      const { router, validToken, llmRouter } = await createTestSetup();

      // Mock LLM response with an explicit citation
      vi.mocked(llmRouter.chatCompletion).mockResolvedValue({
        response: {
          id: 'chatcmpl-test',
          choices: [
            {
              message: {
                role: 'assistant',
                content: 'Based on the knowledge, here is the answer [from: block-1].',
              },
            },
          ],
          usage: { total_tokens: 20 },
        },
        latencyMs: 150,
      });

      const handlers = getHandler(router, 'post', '/chat/completions')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        agentIdentity: {
          agentId: 'test-agent',
          circleId: 'test-circle',
          scope: 'agent',
        },
        body: {
          messages: [{ role: 'user', content: 'What is the knowledge?' }],
        },
      });

      await executeHandlers(handlers, req, res);

      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          citations: [{ blockId: 'block-1', scope: 'personal:agent-1', relevance: 0.95 }],
        })
      );
    });

    it('should include citations based on content overlap when no explicit citation is present', async () => {
      const { router, validToken, llmRouter } = await createTestSetup();

      // Mock LLM response with content overlap but no explicit citation
      // The overlap heuristic checks if a 50-char fragment of the block content is in the response.
      const overlapContent =
        'This knowledge content should trigger overlap detection because it is long.';
      const responseContent =
        'The agent says: This knowledge content should trigger overlap detection because it is long.';

      vi.spyOn(ContextAssembler.prototype, 'assemble').mockImplementation(
        async (agentId, messages, onEvent) => {
          onEvent?.({
            stage: 'memory.retrieved',
            detail: {
              blocks: [
                {
                  id: 'block-overlap',
                  source: 'personal:agent-1',
                  relevance: 0.88,
                  content: overlapContent,
                },
              ],
            },
          });
          return messages;
        }
      );

      vi.mocked(llmRouter.chatCompletion).mockResolvedValue({
        response: {
          id: 'chatcmpl-test',
          object: 'chat.completion',
          created: 1234567890,
          model: 'test-model',
          choices: [
            {
              index: 0,
              message: {
                role: 'assistant',
                content: responseContent,
              },
              finish_reason: 'stop',
            },
          ],
          usage: { prompt_tokens: 10, completion_tokens: 10, total_tokens: 20 },
        },
        latencyMs: 150,
      });

      const handlers = getHandler(router, 'post', '/chat/completions')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

      const { req, res } = createMockReqRes({
        headers: { authorization: `Bearer ${validToken}` },
        agentIdentity: {
          agentId: 'test-agent',
          circleId: 'test-circle',
          scope: 'agent',
        },
        body: {
          messages: [{ role: 'user', content: 'What is the knowledge?' }],
        },
      });

      await executeHandlers(handlers, req, res);

      const jsonResponse = vi.mocked(res.json).mock.calls[0][0];
      expect(jsonResponse).toBeDefined();
      expect(jsonResponse.citations).toBeDefined();
      expect(jsonResponse.citations).toContainEqual({
        blockId: 'block-overlap',
        scope: 'personal:agent-1',
        relevance: 0.88,
      });
    });

    it('should return 503 when circuit breaker is open', async () => {
      const { router, circuitBreakerService, validToken } = await createTestSetup();
      // Make circuit breaker throw CIRCUIT_OPEN error
      const err = new Error('Provider lmstudio is currently unavailable (circuit open)');
      (err as unknown as { code: string }).code = 'CIRCUIT_OPEN';
      (err as unknown as { provider: string }).provider = 'lmstudio';
      vi.spyOn(circuitBreakerService, 'call').mockRejectedValue(err);

      const handlers = getHandler(router, 'post', '/chat/completions')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

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
      const handlers = getHandler(router, 'get', '/models')! as unknown as Array<
        (
          req: import('express').Request,
          res: import('express').Response,
          next: import('express').NextFunction
        ) => void | Promise<void>
      >;

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
