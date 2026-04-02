import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ContextAssembler } from './ContextAssembler.js';
import { Orchestrator } from '../agents/Orchestrator.js';
import { AgentFactory } from '../agents/AgentFactory.js';
import { IdentityService } from '../agents/identity/IdentityService.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { VectorService } from '../services/vector.service.js';
import { SkillInjector } from '../skills/SkillInjector.js';

vi.mock('../skills/SkillInjector.js');
vi.mock('../services/vector.service.js');
vi.mock('../services/embedding.service.js');
vi.mock('../agents/AgentFactory.js');
vi.mock('../agents/identity/IdentityService.js');
vi.mock('../agents/Orchestrator.js');

describe('ContextAssembler', () => {
  let assembler: ContextAssembler;
  let mockOrchestrator: any;
  let mockPool: any;

  beforeEach(() => {
    mockOrchestrator = {
      getManifestByInstanceId: vi.fn(),
      getManifest: vi.fn(),
      getToolExecutor: vi.fn(),
    };
    mockPool = {
      query: vi.fn().mockResolvedValue({ rows: [] }),
    };
    assembler = new ContextAssembler(mockPool, mockOrchestrator as unknown as Orchestrator);

    // Default mocks
    vi.mocked(IdentityService.generateStreamingSystemPrompt).mockReturnValue('Base Prompt');
    vi.mocked(SkillInjector.prototype.inject).mockResolvedValue('Prompt with Skills');

    const mockEmbeddingService = {
      isAvailable: vi.fn().mockReturnValue(true),
      embed: vi.fn().mockResolvedValue([0.1, 0.2]),
      getInstance: vi.fn().mockReturnThis(),
    };
    vi.mocked(EmbeddingService.getInstance).mockReturnValue(mockEmbeddingService as any);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it('should assemble context correctly without memory', async () => {
    const manifest = {
      metadata: { name: 'Test Agent' },
      skills: [],
    };
    mockOrchestrator.getManifest.mockReturnValue(manifest);
    vi.mocked(AgentFactory.getInstance).mockResolvedValue({ circle_id: 'circle-1' } as any);
    mockPool.query.mockResolvedValue({ rows: [{ constitution: 'Circle Constitution' }] });

    const messages = [
      { role: 'system', content: 'fallback' },
      { role: 'user', content: 'hello' },
    ];
    const result = await assembler.assemble('agent-1', messages as any);

    expect(result[0]!.content).toBe('Prompt with Skills');
    expect(IdentityService.generateStreamingSystemPrompt).toHaveBeenCalled();
    expect(SkillInjector.prototype.inject).toHaveBeenCalled();
  });

  it('should skip assembly if manifest not found', async () => {
    mockOrchestrator.getManifest.mockReturnValue(null);
    const messages = [{ role: 'system', content: 'fallback' }];
    const result = await assembler.assemble('agent-1', messages as any);
    expect(result).toEqual(messages);
  });

  it('should inject memory when configured', async () => {
    const manifest = {
      metadata: { name: 'Test Agent' },
      memory: { enabled: true },
    };
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
      } as any;
    });
    // Need to re-instantiate assembler because VectorService is instantiated in constructor
    assembler = new ContextAssembler(mockPool, mockOrchestrator as unknown as Orchestrator);

    const messages = [
      { role: 'system', content: 'fallback' },
      { role: 'user', content: 'hello' },
    ];
    const result = await assembler.assemble('agent-1', messages as any);

    expect(result[0]!.content).toContain('Prompt with Skills');
    expect(result[0]!.content).toContain('<injected_memory>');
    expect(result[0]!.content).toContain('<memory source="personal:agent-1" id="1" relevance="0.900">Memory Content</memory>');
  });

  it('should handle missing system message', async () => {
    const messages = [{ role: 'user', content: 'hello' }];
    const result = await assembler.assemble('agent-1', messages as any);
    expect(result).toEqual(messages);
  });
});
