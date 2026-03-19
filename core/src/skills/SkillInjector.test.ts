import { describe, it, expect, beforeEach, vi } from 'vitest';
import { SkillInjector } from './SkillInjector.js';
import { SkillLibrary } from './SkillLibrary.js';

vi.mock('./SkillLibrary.js', () => ({
  SkillLibrary: {
    getInstance: vi.fn(),
  },
}));

describe('SkillInjector', () => {
  let injector: SkillInjector;
  let poolMock: any;
  let libMock: any;

  beforeEach(() => {
    poolMock = { query: vi.fn().mockResolvedValue({ rows: [] }) };
    libMock = {
      listSkills: vi.fn().mockResolvedValue([]),
      getSkill: vi.fn(),
      getPackage: vi.fn().mockResolvedValue(null),
    };
    (SkillLibrary.getInstance as any).mockReturnValue(libMock);
    injector = new SkillInjector(poolMock);
  });

  it('should inject a declared skill', async () => {
    const skillDoc = {
      name: 'test-skill',
      version: '1.0.0',
      content: 'Skill content',
      triggers: [],
    };
    libMock.getSkill.mockResolvedValue(skillDoc);

    const prompt = '## Guiding Principles\n- Be helpful.\n\n## Context';
    const result = await injector.inject(prompt, ['test-skill'], [], 'user message');

    expect(result).toContain('<skills>');
    expect(result).toContain('Skill content');
    expect(result).toContain('name="test-skill"');
  });

  it('should auto-trigger a skill based on message content', async () => {
    const skillInfo = {
      name: 'git-skill',
      triggers: ['git', 'commit'],
    };
    const skillDoc = {
      ...skillInfo,
      version: '1.0.0',
      content: 'Git guidance',
    };
    libMock.listSkills.mockResolvedValue([skillInfo]);
    libMock.getSkill.mockResolvedValue(skillDoc);

    const prompt = '## Guiding Principles\n- Be helpful.';
    const result = await injector.inject(prompt, [], [], 'I need to commit my changes');

    expect(result).toContain('Git guidance');
    expect(result).toContain('name="git-skill"');
  });

  it('should drop skills when budget is exceeded', async () => {
    // 100 tokens approx 400 chars
    const skill1 = { name: 's1', version: '1', content: 'A'.repeat(400), triggers: [] };
    const skill2 = { name: 's2', version: '1', content: 'B'.repeat(4000), triggers: [] };
    
    libMock.getSkill.mockImplementation((name: string) => {
      if (name === 's1') return Promise.resolve(skill1);
      if (name === 's2') return Promise.resolve(skill2);
      return Promise.resolve(null);
    });

    const prompt = '## Guiding Principles\n- Be helpful.';
    // Budget 500 tokens — circleId is null, tokenBudget is 500
    const result = await injector.inject(prompt, ['s1', 's2'], [], 'msg', null, 500);

    // s1 is ~200 tokens (content + overhead), s2 is ~1100 tokens.
    // s1 should fit in 500 tokens, s2 should not.
    expect(result).toContain('s1');
    expect(result).not.toContain('s2');
  });
});

describe('SkillInjector — constitution injection (Story 10.2)', () => {
  let constitutionLibMock: any;

  beforeEach(() => {
    constitutionLibMock = {
      listSkills: vi.fn().mockResolvedValue([]),
      getSkill: vi.fn().mockResolvedValue(null),
      getPackage: vi.fn().mockResolvedValue(null),
    };
    (SkillLibrary.getInstance as any).mockReturnValue(constitutionLibMock);
  });

  function makeInjector(constitutionOrNull: string | null) {
    const poolMock = {
      query: vi.fn().mockResolvedValue({
        rows: constitutionOrNull !== null ? [{ constitution: constitutionOrNull }] : [],
      }),
    };
    return { injector: new SkillInjector(poolMock as any), poolMock };
  }

  it('injects constitution block when circle has one', async () => {
    const { injector } = makeInjector('# Our Circle\n\nWe value honesty.');
    const result = await injector.inject('Base prompt.', [], [], '', 'circle-uuid-1');
    expect(result).toContain('<circle-constitution>');
    expect(result).toContain('We value honesty.');
    expect(result).toContain('</circle-constitution>');
  });

  it('does not inject when circle constitution is null', async () => {
    const { injector } = makeInjector(null);
    const result = await injector.inject('Base prompt.', [], [], '', 'circle-no-constitution');
    expect(result).not.toContain('<circle-constitution>');
    expect(result).toBe('Base prompt.');
  });

  it('does not query DB when no circleId is provided', async () => {
    const { injector, poolMock } = makeInjector('# ignored');
    await injector.inject('Base prompt.', [], [], '');
    expect(poolMock.query).not.toHaveBeenCalled();
  });

  it('truncates oversized constitution from the bottom, preserving opening statement', async () => {
    const lines = ['# Our Circle', ''];
    for (let i = 0; i < 200; i++) {
      lines.push(`Line ${i + 1}: Important governance rule that is very long and detailed.`);
    }
    const longConstitution = lines.join('\n');

    const { injector } = makeInjector(longConstitution);
    const result = await injector.inject('Base prompt.', [], [], '', 'circle-big');

    expect(result).toContain('<circle-constitution>');
    expect(result).toContain('# Our Circle');
    expect(result).toContain('[truncated for token budget]');
    const match = result.match(/<circle-constitution>([\s\S]*?)<\/circle-constitution>/);
    expect(match).not.toBeNull();
    expect(match![1]!.length).toBeLessThan(longConstitution.length);
  });

  it('truncates large constitution but preserves opening statement', async () => {
    const largeConstitution = '# Circle Purpose\n' + 'A'.repeat(8100);
    const { injector } = makeInjector(largeConstitution);
    const result = await injector.inject('Prompt.', [], [], '', 'circle-warn');
    expect(result).toContain('<circle-constitution>');
    expect(result).toContain('# Circle Purpose');
  });
});
