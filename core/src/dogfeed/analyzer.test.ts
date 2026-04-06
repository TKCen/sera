import { describe, it, expect, vi, beforeEach } from 'vitest';
import { DogfeedAnalyzer } from './analyzer.js';
import fs from 'node:fs';

vi.mock('node:fs');

const SAMPLE_TASK_FILE = `# Dogfeed Task Tracker

Tasks for building SERA's autonomous self-improvement loop.

## Format
\`- [ ] P<priority> | <category> | <description>\`

## Ready (unblocked, pick from top)

### Loop Infrastructure
- [ ] P0 | infra | Build core/src/dogfeed/loop.ts
- [ ] P1 | infra | Add POST /api/dogfeed/run route

### Validation Fuel
- [ ] P1 | lint | Remove unused import in core/src/agents/Orchestrator.ts:2
- [ ] P2 | lint | Replace any type in core/src/lib/database.ts:4

## In Progress

## Done
<!-- Format: - [x] P<n> | <cat> | <description> | <outcome> | <tokens> | <duration> -->
- [x] P0 | infra | Create types.ts | OK | ~2k tokens | 5min
`;

describe('DogfeedAnalyzer', () => {
  let analyzer: DogfeedAnalyzer;

  beforeEach(() => {
    vi.resetAllMocks();
    analyzer = new DogfeedAnalyzer('/fake/DOGFEED-TASKS.md');
  });

  describe('scanTaskFile', () => {
    it('parses ready tasks from the markdown file', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(SAMPLE_TASK_FILE);

      const tasks = analyzer.scanTaskFile();
      const ready = tasks.filter((t) => t.status === 'ready');

      expect(ready).toHaveLength(4);
      expect(ready[0]).toEqual(
        expect.objectContaining({
          priority: 0,
          category: 'infra',
          description: 'Build core/src/dogfeed/loop.ts',
          status: 'ready',
          filePath: 'core/src/dogfeed/loop.ts',
        })
      );
    });

    it('parses done tasks from the Done section', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(SAMPLE_TASK_FILE);

      const tasks = analyzer.scanTaskFile();
      const done = tasks.filter((t) => t.status === 'done');

      expect(done).toHaveLength(1);
      expect(done[0]).toEqual(
        expect.objectContaining({
          priority: 0,
          category: 'infra',
          status: 'done',
        })
      );
    });

    it('extracts file path and line number hints', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(SAMPLE_TASK_FILE);

      const tasks = analyzer.scanTaskFile();
      const lintTask = tasks.find((t) => t.description.includes('Orchestrator'));

      expect(lintTask).toEqual(
        expect.objectContaining({
          filePath: 'core/src/agents/Orchestrator.ts',
          line: 2,
        })
      );
    });

    it('returns empty array when file does not exist', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);

      const tasks = analyzer.scanTaskFile();
      expect(tasks).toEqual([]);
    });
  });

  describe('pickNext', () => {
    it('returns the highest-priority ready task', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(SAMPLE_TASK_FILE);

      const task = analyzer.pickNext();

      expect(task).toBeDefined();
      expect(task!.priority).toBe(0);
      expect(task!.category).toBe('infra');
    });

    it('returns undefined when no ready tasks', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(`# Dogfeed Task Tracker

## Ready

## Done
`);

      const task = analyzer.pickNext();
      expect(task).toBeUndefined();
    });
  });

  describe('markDone', () => {
    it('updates the task file', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.readFileSync).mockReturnValue(SAMPLE_TASK_FILE);
      vi.mocked(fs.writeFileSync).mockImplementation(() => {});

      const task = {
        priority: 1,
        category: 'lint' as const,
        description: 'Remove unused import in core/src/agents/Orchestrator.ts:2',
        status: 'ready' as const,
      };

      analyzer.markDone(task, 'OK', 5, '3min');

      expect(fs.writeFileSync).toHaveBeenCalledOnce();
      const written = vi.mocked(fs.writeFileSync).mock.calls[0]![1] as string;
      expect(written).toContain('- [x] P1 | lint | Remove unused import');
      expect(written).toContain('OK');
    });
  });
});
