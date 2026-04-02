import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { Pool } from 'pg';
import { ContextAssembler } from './ContextAssembler.js';
import { Orchestrator, AgentFactory, IdentityService } from '../agents/index.js';
import type { AgentManifest } from '../agents/manifest/types.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { VectorService } from '../services/vector.service.js';
import { SkillInjector } from '../skills/index.js';

vi.mock('../skills/index.js');
vi.mock('../services/vector.service.js');
vi.mock('../services/embedding.service.js');
vi.mock('../agents/index.js');

describe('ContextAssembler', () => {
  let assembler: ContextAssembler;
  let mockOrchestrator: Orchestrator;
  let mockPool: Pool;

  beforeEach(() => {
    mockOrchestrator = {
      getManifestByInstanceId: vi.fn(),
      getManifest: vi.fn(),
      getToolExecutor: vi.fn(),
    } as unknown as Orchestrator;
    mockPool = {
      query: vi.fn().mockResolvedValue({ rows: [] }),
    } as unknown as Pool;
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
    };
    vi.mocked(mockOrchestrator.getManifest).mockReturnValue(manifest as unknown as AgentManifest);
    vi.mocked(AgentFactory.getInstance).mockResolvedValue({
      circle_id: 'circle-1',
    } as never);
    vi.mocked(mockPool.query).mockResolvedValue({
      rows: [{ constitution: 'Circle Constitution' }],
    } as never);

    const messages = [
      { role: 'system', content: 'fallback' },
      { role: 'user', content: 'hello' },
    ] as unknown as never[];
    const result = await assembler.assemble('agent-1', messages);

    expect(result[0]!.content).toBe('Prompt with Skills');
    expect(IdentityService.generateStreamingSystemPrompt).toHaveBeenCalled();
    expect(SkillInjector.prototype.inject).toHaveBeenCalled();
  });

  it('should skip assembly if manifest not found', async () => {
    vi.mocked(mockOrchestrator.getManifest).mockReturnValue(undefined);
    const messages = [{ role: 'system', content: 'fallback' }] as unknown as never[];
    const result = await assembler.assemble('agent-1', messages);
    expect(result).toEqual(messages);
  });

  it('should inject memory when configured', async () => {
    const manifest = {
      metadata: { name: 'Test Agent' },
      memory: { enabled: true },
    };
    vi.mocked(mockOrchestrator.getManifest).mockReturnValue(manifest as unknown as AgentManifest);

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

    const messages = [
      { role: 'system', content: 'fallback' },
      { role: 'user', content: 'hello' },
    ] as unknown as never[];
    const result = await assembler.assemble('agent-1', messages);

    expect(result[0]!.content).toContain('Prompt with Skills');
    expect(result[0]!.content).toContain('<injected_memory>');
    expect(result[0]!.content).toContain(
      '<memory source="personal:agent-1" id="1" relevance="0.900">Memory Content</memory>'
    );
  });

  it('should handle missing system message', async () => {
    const messages = [{ role: 'user', content: 'hello' }] as unknown as never[];
    const result = await assembler.assemble('agent-1', messages);
    expect(result).toEqual(messages);
  });
});
