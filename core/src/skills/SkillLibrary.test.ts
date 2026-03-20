import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { SkillLibrary } from './SkillLibrary.js';
import path from 'node:path';
import fs from 'node:fs/promises';
import os from 'node:os';

describe('SkillLibrary', () => {
  let tmpDir: string;
  let poolMock: any;

  beforeEach(async () => {
    SkillLibrary.resetInstance();
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'sera-skills-test-'));
    poolMock = {
      query: vi.fn().mockResolvedValue({ rows: [], rowCount: 0 }),
    };
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
  });

  it('should parse a valid skill file', async () => {
    const skillPath = path.join(tmpDir, 'test-skill.md');
    await fs.writeFile(
      skillPath,
      `---
id: test-skill
name: Test Skill
version: 1.0.0
description: A test skill
triggers: ["test"]
---
# Content
`
    );

    const lib = SkillLibrary.getInstance(poolMock);
    // @ts-ignore - accessing private method for testing
    const doc = await lib.parseSkillFile(skillPath, 'external');

    expect(doc).not.toBeNull();
    expect(doc?.name).toBe('Test Skill');
    expect(doc?.content).toBe('# Content');
    expect(doc?.source).toBe('external');
  });

  it('should return null for invalid front-matter', async () => {
    const skillPath = path.join(tmpDir, 'invalid-skill.md');
    await fs.writeFile(
      skillPath,
      `---
invalid: true
---
# Content
`
    );

    const lib = SkillLibrary.getInstance(poolMock);
    // @ts-ignore
    const doc = await lib.parseSkillFile(skillPath, 'external');

    expect(doc).toBeNull();
  });

  it('should upsert a skill to the database', async () => {
    const lib = SkillLibrary.getInstance(poolMock);
    const doc = {
      id: 'test-skill',
      name: 'Test Skill',
      version: '1.0.0',
      description: 'A test skill',
      triggers: ['test'],
      content: '# Content',
      source: 'bundled' as const,
    };

    // @ts-ignore
    await lib.upsertSkill(doc);

    expect(poolMock.query).toHaveBeenCalledWith(
      expect.stringContaining('INSERT INTO skills'),
      expect.arrayContaining(['Test Skill', '1.0.0', '# Content', 'bundled'])
    );
  });

  it('should watch for changes and reload skills', async () => {
    const lib = SkillLibrary.getInstance(poolMock);
    const intercomMock = { publish: vi.fn().mockResolvedValue(undefined) };
    lib.setIntercom(intercomMock as any);

    // Mock search paths to use tmpDir
    vi.spyOn(lib as any, 'getSearchPaths').mockReturnValue([{ path: tmpDir, source: 'external' }]);

    lib.watchSkills();

    // Give chokidar some time to start
    await new Promise((resolve) => setTimeout(resolve, 500));

    // Create a new skill file
    const skillPath = path.join(tmpDir, 'new-skill.md');
    const skillContent = `---
name: New Skill
version: 1.1.0
description: Hot reload test
triggers: ["hot"]
---
# New Content`;

    await fs.writeFile(skillPath, skillContent);

    // Wait for chokidar to pick up the change (DoD says within 2s)
    for (let i = 0; i < 4; i++) {
      await new Promise((resolve) => setTimeout(resolve, 500));
      if (poolMock.query.mock.calls.length > 0) break;
    }

    expect(poolMock.query).toHaveBeenCalledWith(
      expect.stringContaining('INSERT INTO skills'),
      expect.arrayContaining(['New Skill', '1.1.0'])
    );
    expect(intercomMock.publish).toHaveBeenCalledWith(
      'system.skill-reloaded',
      expect.objectContaining({ name: 'New Skill', version: '1.1.0' })
    );

    lib.stopWatching();
  });
});
