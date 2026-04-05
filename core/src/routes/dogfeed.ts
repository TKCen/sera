/**
 * Dogfeed routes — REST API for triggering and monitoring dogfeed cycles.
 *
 * Phase 0 bootstrap: TypeScript orchestration that proves the loop works.
 * Phase 1+: This moves to sera-core-rs (Rust) as native SERA orchestration.
 */

import { Router } from 'express';
import { DogfeedLoop } from '../dogfeed/loop.js';
import { DogfeedAnalyzer } from '../dogfeed/analyzer.js';
import { createDefaultConfig } from '../dogfeed/constants.js';
import type { DogfeedConfig } from '../dogfeed/types.js';

export const createDogfeedRouter = (configOverrides?: Partial<DogfeedConfig>): Router => {
  const router = Router();
  const config = createDefaultConfig(configOverrides);
  const loop = new DogfeedLoop(configOverrides);
  const analyzer = new DogfeedAnalyzer(config.taskFile);

  let running = false;

  /**
   * POST /api/dogfeed/run — Trigger a dogfeed cycle.
   *
   * Runs one cycle: pick task → spawn agent → verify CI → merge.
   * Returns the cycle result. Only one cycle can run at a time.
   */
  router.post('/run', async (_req, res) => {
    if (running) {
      res.status(409).json({
        error: 'A dogfeed cycle is already running',
        status: loop.getStatus(),
      });
      return;
    }

    running = true;
    try {
      const result = await loop.runCycle();
      res.json(result);
    } catch (err: unknown) {
      res.status(500).json({
        error: 'Dogfeed cycle failed unexpectedly',
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      running = false;
    }
  });

  /**
   * GET /api/dogfeed/status — Get current cycle status.
   */
  router.get('/status', (_req, res) => {
    res.json({
      running,
      status: loop.getStatus(),
      lastResult: loop.getLastResult(),
    });
  });

  /**
   * GET /api/dogfeed/tasks — List available dogfeed tasks.
   */
  router.get('/tasks', (_req, res) => {
    const tasks = analyzer.scanTaskFile();
    const ready = tasks.filter((t) => t.status === 'ready');
    const done = tasks.filter((t) => t.status === 'done');
    res.json({ total: tasks.length, ready: ready.length, done: done.length, tasks });
  });

  /**
   * GET /api/dogfeed/next — Preview the next task that would be picked.
   */
  router.get('/next', (_req, res) => {
    const task = analyzer.pickNext();
    if (!task) {
      res.json({ available: false, task: null });
      return;
    }
    res.json({ available: true, task });
  });

  return router;
};
