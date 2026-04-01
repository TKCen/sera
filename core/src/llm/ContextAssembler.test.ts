import { describe, it, expect, vi, beforeEach, afterEach, type Mocked } from 'vitest';
import { ContextAssembler } from './ContextAssembler.js';
import { Orchestrator } from '../agents/Orchestrator.js';
import { AgentFactory } from '../agents/AgentFactory.js';
import { IdentityService } from '../agents/identity/IdentityService.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { VectorService } from '../services/vector.service.js';
import { SkillInjector } from '../skills/SkillInjector.js';
import type { ChatMessage } from './LlmRouter.js';
import type { AgentManifest } from '../agents/manifest/types.js';

vi.mock('../skills/SkillInjector.js');
vi.mock('../services/vector.service.js');
vi.mock('../services/embedding.service.js');
vi.mock('../agents/AgentFactory.js');
vi.mock('../agents/identity/IdentityService.js');
vi.mock('../agents/Orchestrator.js');

import type { Pool } from 'pg';

describe('ContextAssembler', () => {
  let assembler: ContextAssembler;
  let mockOrchestrator: Mocked<Orchestrator>;
  let mockPool: Mocked<Pool>;

  beforeEach(() => {
    mockOrchestrator = {
      getManifestByInstanceId: vi.fn(),
      getManifest: vi.fn(),
    } as unknown as Mocked<Orchestrator>;
    mockPool = {
      query: vi.fn().mockResolvedValue({ rows: [] }),
    } as unknown as Mocked<Pool>;
    assembler = new ContextAssembler(mockPool, mockOrchestrator);

    // Default mocks
    vi.mocked(IdentityService.generateStreamingSystemPrompt).mockReturnValue('Base Prompt');
    vi.mocked(SkillInjector.prototype.inject).mockResolvedValue('Prompt with Skills');

    const mockEmbeddingService = {
      isAvailable: vi.fn().mockReturnValue(true),
      embed: vi.fn().mockResolvedValue([0.1, 0.2]),
      getInstance: vi.fn().mockReturnThis(),
    };
    vi.mocked(EmbeddingService.getInstance).mockReturnValue(
      mockEmbeddingService as unknown as EmbeddingService
    );
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('should assemble context correctly without memory', async () => {
    const manifest = {
      metadata: { name: 'Test Agent' },
      skills: [],
    } as unknown as AgentManifest;
    mockOrchestrator.getManifest.mockReturnValue(manifest);
    vi.mocked(AgentFactory.getInstance).mockResolvedValue({
      circle_id: 'circle-1',
    } as unknown as import('../agents/types.js').AgentInstance);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (mockPool.query as any).mockResolvedValue({
      rows: [{ constitution: 'Circle Constitution' }],
      command: 'SELECT',
      rowCount: 1,
      oid: 0,
      fields: [],
    });

    const messages: ChatMessage[] = [
      { role: 'system', content: 'fallback' },
      { role: 'user', content: 'hello' },
    ];
    const result = await assembler.assemble('agent-1', messages);

    expect(result[0]!.content).toBe('Prompt with Skills');
    expect(IdentityService.generateStreamingSystemPrompt).toHaveBeenCalled();
    expect(SkillInjector.prototype.inject).toHaveBeenCalled();
  });

  it('should skip assembly if manifest not found', async () => {
    mockOrchestrator.getManifest.mockReturnValue(undefined);
    const messages: ChatMessage[] = [{ role: 'system', content: 'fallback' }];
    const result = await assembler.assemble('agent-1', messages);
    expect(result).toEqual(messages);
  });

  it('should inject memory when configured', async () => {
    const manifest = {
      metadata: { name: 'Test Agent' },
      memory: { enabled: true },
    } as unknown as AgentManifest;
    mockOrchestrator.getManifest.mockReturnValue(manifest);

    const mockVectorSearch = vi.fn().mockResolvedValue([
      {
        id: '1',
        namespace: 'personal:agent-1',
        score: 0.9,
        payload: { content: 'Memory Content', type: 'note' },
      },
    ]);
    vi.mocked(VectorService).mockImplementation(function () {
      return {
        search: mockVectorSearch,
      } as unknown as VectorService;
    });
    // Need to re-instantiate assembler because VectorService is instantiated in constructor
    assembler = new ContextAssembler(mockPool, mockOrchestrator);

    const messages: ChatMessage[] = [
      { role: 'system', content: 'fallback' },
      { role: 'user', content: 'hello' },
    ];
    const result = await assembler.assemble('agent-1', messages);

    expect(result[0]!.content).toContain('Prompt with Skills');
    expect(result[0]!.content).toContain('<memory>');
    expect(result[0]!.content).toContain('Memory Content');
  });

  it('should handle missing system message', async () => {
    const messages: ChatMessage[] = [{ role: 'user', content: 'hello' }];
    const result = await assembler.assemble('agent-1', messages);
    expect(result).toEqual(messages);
  });
});
