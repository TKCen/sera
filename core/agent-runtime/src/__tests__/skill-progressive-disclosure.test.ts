/**
 * Tests for progressive disclosure skill tools:
 *   - buildSkillsMetadataBlock (Tier 1 system prompt injection)
 *   - listSkills (list_skills tool)
 *   - viewSkill (view_skill tool)
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';

// We import the module after setting up env vars so the module-level path
// variables pick up the test directory. We use vi.resetModules() + dynamic
// re-import to get a fresh module instance per test group.

describe('skill-handler — progressive disclosure', () => {
  let skillsDir: string;

  function writeIndex(skills: object[]): void {
    fs.writeFileSync(
      path.join(skillsDir, 'index.json'),
      JSON.stringify({ version: '1.0', generatedAt: new Date().toISOString(), skills }),
      'utf-8'
    );
  }

  beforeEach(async () => {
    skillsDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sera-skills-test-'));
    process.env['SERA_SKILLS_DIR'] = skillsDir;

    // Reset module cache so skill-handler picks up the new env var
    vi.resetModules();
  });

  afterEach(() => {
    delete process.env['SERA_SKILLS_DIR'];
    try {
      fs.rmSync(skillsDir, { recursive: true, force: true });
    } catch {
      // ignore cleanup failures
    }
  });

  // ── buildSkillsMetadataBlock ──────────────────────────────────────────────

  describe('buildSkillsMetadataBlock()', () => {
    it('returns empty string when no index file exists', async () => {
      const { buildSkillsMetadataBlock } = await import('../tools/skill-handler.js');
      const result = buildSkillsMetadataBlock();
      expect(result).toBe('');
    });

    it('returns compact metadata block for available skills', async () => {
      writeIndex([
        { name: 'web-search', description: 'Search the web for information.', version: '1.0' },
        { name: 'code-review', description: 'Review code for issues.', version: '2.0' },
      ]);
      const { buildSkillsMetadataBlock } = await import('../tools/skill-handler.js');
      const result = buildSkillsMetadataBlock();

      expect(result).toContain('Available skills (use view_skill to see full details):');
      expect(result).toContain('- web-search: Search the web for information.');
      expect(result).toContain('- code-review: Review code for issues.');
    });

    it('truncates skill name to 64 chars', async () => {
      const longName = 'a'.repeat(80);
      writeIndex([{ name: longName, description: 'Some description.', version: '1.0' }]);
      const { buildSkillsMetadataBlock } = await import('../tools/skill-handler.js');
      const result = buildSkillsMetadataBlock();

      expect(result).toContain('- ' + 'a'.repeat(64) + ':');
      expect(result).not.toContain('a'.repeat(65));
    });

    it('truncates description to 1024 chars', async () => {
      const longDesc = 'b'.repeat(2000);
      writeIndex([{ name: 'my-skill', description: longDesc, version: '1.0' }]);
      const { buildSkillsMetadataBlock } = await import('../tools/skill-handler.js');
      const result = buildSkillsMetadataBlock();

      expect(result).toContain('- my-skill: ' + 'b'.repeat(1024));
      expect(result).not.toContain('b'.repeat(1025));
    });

    it('returns empty string when index has no skills', async () => {
      writeIndex([]);
      const { buildSkillsMetadataBlock } = await import('../tools/skill-handler.js');
      const result = buildSkillsMetadataBlock();
      expect(result).toBe('');
    });
  });

  // ── listSkills ────────────────────────────────────────────────────────────

  describe('listSkills()', () => {
    it('returns error JSON when no index exists', async () => {
      const { listSkills } = await import('../tools/skill-handler.js');
      const raw = await listSkills();
      const parsed = JSON.parse(raw);
      expect(parsed.error).toContain('No skill index available');
      expect(parsed.skills).toEqual([]);
    });

    it('returns Tier 1 metadata for all skills', async () => {
      writeIndex([
        {
          name: 'web-search',
          displayName: 'Web Search',
          description: 'Search the web.',
          version: '1.0',
          tags: ['research'],
        },
        {
          name: 'code-review',
          displayName: 'Code Review',
          description: 'Review code.',
          version: '2.0',
          tags: ['engineering'],
        },
      ]);
      const { listSkills } = await import('../tools/skill-handler.js');
      const raw = await listSkills();
      const parsed = JSON.parse(raw);

      expect(parsed.total).toBe(2);
      expect(parsed.skills).toHaveLength(2);

      const webSearch = parsed.skills.find((s: { name: string }) => s.name === 'web-search');
      expect(webSearch).toBeDefined();
      expect(webSearch.description).toBe('Search the web.');
      // Should not expose version/tags/parameters (Tier 1 only)
      expect(webSearch.version).toBeUndefined();
      expect(webSearch.tags).toBeUndefined();
    });

    it('truncates name and description in Tier 1 output', async () => {
      const longName = 'x'.repeat(100);
      const longDesc = 'y'.repeat(2000);
      writeIndex([{ name: longName, description: longDesc, version: '1.0' }]);
      const { listSkills } = await import('../tools/skill-handler.js');
      const raw = await listSkills();
      const parsed = JSON.parse(raw);

      expect(parsed.skills[0].name).toHaveLength(64);
      expect(parsed.skills[0].description).toHaveLength(1024);
    });
  });

  // ── viewSkill ─────────────────────────────────────────────────────────────

  describe('viewSkill()', () => {
    it('returns error when name parameter is missing', async () => {
      writeIndex([{ name: 'my-skill', description: 'A skill.', version: '1.0' }]);
      const { viewSkill } = await import('../tools/skill-handler.js');
      const raw = await viewSkill({});
      const parsed = JSON.parse(raw);
      expect(parsed.error).toContain('Missing required parameter: name');
    });

    it('returns error when skill not found in index', async () => {
      writeIndex([{ name: 'other-skill', description: 'Another.', version: '1.0' }]);
      const { viewSkill } = await import('../tools/skill-handler.js');
      const raw = await viewSkill({ name: 'unknown-skill' });
      const parsed = JSON.parse(raw);
      expect(parsed.error).toContain('not found');
      expect(parsed.error).toContain('list_skills');
    });

    it('returns full Tier 2 metadata + (no content) when skill dir has no content file', async () => {
      writeIndex([
        {
          name: 'my-skill',
          displayName: 'My Skill',
          description: 'Does something.',
          version: '1.2.3',
          parameters: { type: 'object', properties: {} },
          tags: ['research'],
        },
      ]);
      const { viewSkill } = await import('../tools/skill-handler.js');
      const raw = await viewSkill({ name: 'my-skill' });
      const parsed = JSON.parse(raw);

      expect(parsed.name).toBe('my-skill');
      expect(parsed.displayName).toBe('My Skill');
      expect(parsed.version).toBe('1.2.3');
      expect(parsed.content).toContain('No content file found');
    });

    it('reads content.md from skill directory (Tier 2)', async () => {
      writeIndex([
        {
          name: 'my-skill',
          displayName: 'My Skill',
          description: 'Does something.',
          version: '1.0',
          parameters: {},
        },
      ]);
      const skillSubDir = path.join(skillsDir, 'my-skill');
      fs.mkdirSync(skillSubDir, { recursive: true });
      fs.writeFileSync(
        path.join(skillSubDir, 'content.md'),
        '# My Skill\nFull instructions here.',
        'utf-8'
      );

      const { viewSkill } = await import('../tools/skill-handler.js');
      const raw = await viewSkill({ name: 'my-skill' });
      const parsed = JSON.parse(raw);

      expect(parsed.content).toContain('# My Skill');
      expect(parsed.content).toContain('Full instructions here.');
    });

    it('falls back to README.md when content.md is absent', async () => {
      writeIndex([
        {
          name: 'readme-skill',
          displayName: 'Readme Skill',
          description: 'Desc.',
          version: '1.0',
          parameters: {},
        },
      ]);
      const skillSubDir = path.join(skillsDir, 'readme-skill');
      fs.mkdirSync(skillSubDir, { recursive: true });
      fs.writeFileSync(path.join(skillSubDir, 'README.md'), '# README content', 'utf-8');

      const { viewSkill } = await import('../tools/skill-handler.js');
      const raw = await viewSkill({ name: 'readme-skill' });
      const parsed = JSON.parse(raw);

      expect(parsed.content).toContain('# README content');
    });

    it('does not include references when include_references is false', async () => {
      writeIndex([
        {
          name: 'my-skill',
          displayName: 'My Skill',
          description: 'Desc.',
          version: '1.0',
          parameters: {},
        },
      ]);
      const skillSubDir = path.join(skillsDir, 'my-skill');
      fs.mkdirSync(path.join(skillSubDir, 'refs'), { recursive: true });
      fs.writeFileSync(path.join(skillSubDir, 'refs', 'ref1.md'), 'ref content', 'utf-8');

      const { viewSkill } = await import('../tools/skill-handler.js');
      const raw = await viewSkill({ name: 'my-skill' });
      const parsed = JSON.parse(raw);

      expect(parsed.references).toBeUndefined();
    });

    it('loads Tier 3 reference files when include_references is true', async () => {
      writeIndex([
        {
          name: 'my-skill',
          displayName: 'My Skill',
          description: 'Desc.',
          version: '1.0',
          parameters: {},
        },
      ]);
      const skillSubDir = path.join(skillsDir, 'my-skill');
      fs.mkdirSync(path.join(skillSubDir, 'refs'), { recursive: true });
      fs.writeFileSync(path.join(skillSubDir, 'content.md'), 'main content', 'utf-8');
      fs.writeFileSync(path.join(skillSubDir, 'refs', 'template.md'), 'template content', 'utf-8');
      fs.writeFileSync(path.join(skillSubDir, 'refs', 'examples.md'), 'examples here', 'utf-8');

      const { viewSkill } = await import('../tools/skill-handler.js');
      const raw = await viewSkill({ name: 'my-skill', include_references: true });
      const parsed = JSON.parse(raw);

      expect(parsed.references).toBeDefined();
      expect(parsed.references).toHaveLength(2);
      const files = parsed.references.map((r: { file: string }) => r.file);
      expect(files).toContain('template.md');
      expect(files).toContain('examples.md');
    });

    it('returns empty references array when refs dir does not exist', async () => {
      writeIndex([
        {
          name: 'norefs-skill',
          displayName: 'No Refs',
          description: 'Desc.',
          version: '1.0',
          parameters: {},
        },
      ]);
      // No refs directory created

      const { viewSkill } = await import('../tools/skill-handler.js');
      const raw = await viewSkill({ name: 'norefs-skill', include_references: true });
      const parsed = JSON.parse(raw);

      expect(parsed.references).toEqual([]);
    });
  });
});
