import { Router } from 'express';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { PipelineService } from '../services/PipelineService.js';
import type { PipelineStep } from '../services/PipelineService.js';
import { IntercomService } from '../intercom/IntercomService.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('PipelinesRouter');

const PARALLEL_DEFAULT_TIMEOUT_MS = 10 * 60 * 1000;

export function createPipelinesRouter(orchestrator: Orchestrator) {
  const router = Router();
  const pipelineService = PipelineService.getInstance();

  // ── POST /api/pipelines ────────────────────────────────────────────────────
  router.post('/', async (req, res) => {
    try {
      const { type, tasks, managerAgent, timeoutMs } = req.body;

      if (!type || !['sequential', 'parallel', 'hierarchical'].includes(type)) {
        return res
          .status(400)
          .json({ error: 'type must be sequential, parallel, or hierarchical' });
      }
      if (!Array.isArray(tasks) || tasks.length === 0) {
        return res.status(400).json({ error: 'tasks must be a non-empty array' });
      }
      if (type === 'hierarchical' && !managerAgent) {
        return res.status(400).json({ error: 'hierarchical pipelines require managerAgent' });
      }

      const steps: PipelineStep[] = tasks.map((t: any) => ({
        agentId: t.assignedAgent,
        description: t.description,
        status: 'pending',
      }));

      const pipeline = await pipelineService.create(type, steps);

      // Run async
      executePipeline(
        pipeline.id,
        type,
        tasks,
        managerAgent,
        timeoutMs ?? PARALLEL_DEFAULT_TIMEOUT_MS,
        orchestrator,
        pipelineService
      ).catch((err) => logger.error(`Pipeline ${pipeline.id} failed:`, err));

      res.status(202).json(pipeline);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // ── GET /api/pipelines/:id ─────────────────────────────────────────────────
  router.get('/:id', async (req, res) => {
    try {
      const pipeline = await pipelineService.get(req.params.id!);
      if (!pipeline) return res.status(404).json({ error: 'Pipeline not found' });
      res.json(pipeline);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}

async function executePipeline(
  pipelineId: string,
  type: 'sequential' | 'parallel' | 'hierarchical',
  tasks: Array<{ id: string; description: string; assignedAgent?: string }>,
  managerAgentName: string | undefined,
  timeoutMs: number,
  orchestrator: Orchestrator,
  pipelineService: PipelineService
): Promise<void> {
  await pipelineService.updateStatus(pipelineId, 'running');
  const intercom = orchestrator.getIntercom();

  try {
    const result = await orchestrator.executeWithProcess(type, tasks, managerAgentName);

    const finalSteps: PipelineStep[] = result.results.map((r) => ({
      agentId: r.agentName,
      description: tasks.find((t) => t.id === r.taskId)?.description ?? r.taskId,
      status: r.status === 'completed' ? ('completed' as const) : ('failed' as const),
      result: r.output,
      ...(r.error !== undefined ? { error: r.error } : {}),
      completedAt: new Date().toISOString(),
    }));

    await pipelineService.updateSteps(pipelineId, finalSteps);

    const overallStatus = finalSteps.some((s) => s.status === 'failed') ? 'failed' : 'completed';
    await pipelineService.updateStatus(pipelineId, overallStatus);

    if (intercom) {
      await intercom
        .publish('system.agents', {
          type: 'pipeline.completed',
          pipelineId,
          status: overallStatus,
          timestamp: new Date().toISOString(),
        })
        .catch(() => {});
    }

    logger.info(`Pipeline ${pipelineId} ${overallStatus}`);
  } catch (err) {
    await pipelineService.updateStatus(pipelineId, 'failed');
    if (intercom) {
      await intercom
        .publish('system.agents', {
          type: 'pipeline.failed',
          pipelineId,
          error: (err as Error).message,
          timestamp: new Date().toISOString(),
        })
        .catch(() => {});
    }
    logger.error(`Pipeline ${pipelineId} failed:`, err);
  }
}
