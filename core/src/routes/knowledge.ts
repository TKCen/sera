/**
 * Epic 8 — Knowledge routes for git-backed circle/global knowledge management.
 */

import { Router } from 'express';
import { KnowledgeGitService } from '../memory/KnowledgeGitService.js';
import type { MergeStrategy } from '../memory/KnowledgeGitService.js';
import type { LlmRouter } from '../llm/LlmRouter.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('KnowledgeRouter');

const VALID_STRATEGIES: ReadonlySet<MergeStrategy> = new Set(['ours', 'theirs', 'llm']);

/**
 * Build an LLM-powered merge function using the platform's default model.
 * The LLM is given both versions and asked to produce a unified result.
 */
function buildLlmMergeFn(llmRouter: LlmRouter) {
  return async (ours: string, theirs: string, filePath: string): Promise<string> => {
    const defaultModel = llmRouter.getRegistry().getDefaultModel();
    if (!defaultModel) {
      throw new Error('No default model configured — cannot perform LLM-assisted merge');
    }

    const { response } = await llmRouter.chatCompletion(
      {
        model: defaultModel,
        messages: [
          {
            role: 'system',
            content:
              'You are a knowledge merge assistant. You are given two versions of a knowledge ' +
              'document that have conflicting edits. Produce a single merged version that ' +
              'preserves all unique information from both versions. Keep the same format ' +
              '(markdown/YAML frontmatter if present). Output ONLY the merged document ' +
              'content — no explanation, no code fences, no commentary.',
          },
          {
            role: 'user',
            content:
              `File: ${filePath}\n\n` +
              `=== VERSION A (current main) ===\n${ours}\n\n` +
              `=== VERSION B (agent branch) ===\n${theirs}\n\n` +
              'Produce the merged document:',
          },
        ],
        temperature: 0.1,
      },
      'system:knowledge-merge'
    );

    const merged = response.choices[0]?.message?.content;
    if (!merged) throw new Error('LLM returned empty merge result');
    return merged;
  };
}

export function createKnowledgeRouter(llmRouter?: LlmRouter): Router {
  const router = Router();
  const gitService = KnowledgeGitService.getInstance();

  /** GET /api/knowledge/circles/:id/history */
  router.get('/circles/:id/history', async (req, res) => {
    try {
      const log = await gitService.log(req.params.id!);
      res.json(log);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  /** GET /api/knowledge/circles/:id/merge-requests */
  router.get('/circles/:id/merge-requests', async (req, res) => {
    try {
      const requests = await gitService.listMergeRequests(req.params.id!);
      res.json(requests);
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  /** POST /api/knowledge/circles/:id/merge-requests/:requestId/approve */
  router.post('/circles/:id/merge-requests/:requestId/approve', async (req, res) => {
    try {
      const reqWithIdentity = req as unknown as { identity?: { id?: string } };
      const approvedBy = reqWithIdentity.identity?.id ?? 'operator';
      await gitService.approveMergeRequest(req.params.requestId!, approvedBy);
      res.json({ success: true });
    } catch (err: unknown) {
      res.status(500).json({ error: err instanceof Error ? err.message : String(err) });
    }
  });

  /**
   * POST /api/knowledge/circles/:id/merge-requests/:requestId/resolve
   * Conflict resolution — accept ours, theirs, or use LLM-assisted merge.
   */
  router.post('/circles/:id/merge-requests/:requestId/resolve', async (req, res) => {
    try {
      const { strategy } = req.body as { strategy: string };

      if (!strategy || !VALID_STRATEGIES.has(strategy as MergeStrategy)) {
        res.status(400).json({
          error: `Invalid strategy "${strategy}". Must be one of: ours, theirs, llm`,
        });
        return;
      }

      const mergeStrategy = strategy as MergeStrategy;

      if (mergeStrategy === 'llm' && !llmRouter) {
        res.status(503).json({
          error: 'LLM-assisted merge is unavailable — no LLM router configured',
        });
        return;
      }

      const reqWithIdentity = req as unknown as { identity?: { id?: string } };
      const resolvedBy = reqWithIdentity.identity?.id ?? 'operator';

      const llmMergeFn =
        mergeStrategy === 'llm' && llmRouter ? buildLlmMergeFn(llmRouter) : undefined;

      const resolution = await gitService.resolveMergeConflict(
        req.params.requestId!,
        mergeStrategy,
        resolvedBy,
        llmMergeFn
      );

      logger.info(
        `Merge conflict resolved: MR=${req.params.requestId} strategy=${mergeStrategy} ` +
          `files=${resolution.filesResolved.length}`
      );

      res.json({
        success: true,
        strategy: resolution.strategy,
        filesResolved: resolution.filesResolved,
        commitHash: resolution.commitHash,
      });
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      logger.error(`Merge conflict resolution failed: ${message}`);
      res.status(500).json({ error: message });
    }
  });

  return router;
}
