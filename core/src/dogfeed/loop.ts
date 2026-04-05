/**
 * DogfeedLoop — orchestrates a complete dogfeed cycle:
 *   analyze → branch → execute (Docker) → pull → verify CI → merge → record
 *
 * The coding agent runs in a Docker container with a fresh clone.
 * CI runs on the host (has node_modules). Merge happens on the host.
 */

import fs from 'node:fs';
import { Logger } from '../lib/logger.js';
import type { DogfeedConfig, DogfeedTask, DogfeedCycleResult, CycleStatus } from './types.js';
import { createDefaultConfig, DOGFEED_BRANCH_PREFIX, DOGFEED_CO_AUTHOR } from './constants.js';
import { DogfeedAnalyzer } from './analyzer.js';
import { AgentSpawner } from './agent-spawner.js';
import { VerifyMerge } from './verify-merge.js';

const logger = new Logger('DogfeedLoop');

export class DogfeedLoop {
  private config: DogfeedConfig;
  private analyzer: DogfeedAnalyzer;
  private spawner: AgentSpawner;
  private verifier: VerifyMerge;
  private currentStatus: CycleStatus = { phase: 'idle' };
  private lastResult?: DogfeedCycleResult;
  private cycleCount = 0;

  constructor(configOverrides?: Partial<DogfeedConfig>) {
    this.config = createDefaultConfig(configOverrides);
    this.analyzer = new DogfeedAnalyzer(this.config.taskFile);
    this.spawner = new AgentSpawner(this.config);
    this.verifier = new VerifyMerge(this.config);
  }

  getStatus(): CycleStatus {
    return { ...this.currentStatus };
  }

  getLastResult(): DogfeedCycleResult | undefined {
    return this.lastResult;
  }

  /**
   * Run a complete dogfeed cycle.
   *
   * Flow:
   *   1. Pick task from tracker
   *   2. Create branch on host and push
   *   3. Spawn Docker container (clones repo, agent edits, commits, pushes)
   *   4. Pull branch on host
   *   5. Merge into current working tree (stash/unstash around it)
   *   6. Run CI on host (has node_modules)
   *   7. If CI fails → revert merge. If passes → push + record.
   */
  async runCycle(): Promise<DogfeedCycleResult> {
    const startTime = Date.now();

    // ── 1. Analyze — pick task ────────────────────────────────────────────────
    this.updateStatus({ phase: 'analyzing' });
    const task = this.analyzer.pickNext();

    if (!task) {
      const result = this.buildEmptyResult(startTime);
      this.updateStatus({ phase: 'idle' });
      return result;
    }

    const agent = this.spawner.routeTask(task);
    const branchSlug = task.description
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .substring(0, 40)
      .replace(/-+$/, '');
    const branchName = `${DOGFEED_BRANCH_PREFIX}${++this.cycleCount}-${branchSlug}`;

    this.updateStatus({ phase: 'analyzing', task, agent, branch: branchName });
    logger.info(`Cycle ${this.cycleCount}: ${task.description} → ${agent}`);

    try {
      // ── 2. Create branch on host ────────────────────────────────────────────
      this.updateStatus({ phase: 'branching', task, agent, branch: branchName });
      await this.verifier.createBranch(branchName);

      // ── 3. Execute — spawn coding agent in Docker ───────────────────────────
      this.updateStatus({
        phase: 'executing',
        task,
        agent,
        branch: branchName,
        startedAt: new Date().toISOString(),
      });
      const agentResult = await this.spawner.spawn(task, branchName);

      logger.info(`Agent finished (exit ${agentResult.exitCode}, ${agentResult.durationMs}ms)`);
      if (agentResult.output) {
        logger.info(`Agent output: ${agentResult.output.substring(0, 500)}`);
      }

      if (!agentResult.success) {
        return this.handleFailure(
          task,
          agent,
          branchName,
          startTime,
          agentResult.output,
          agentResult.timedOut
            ? 'Agent timed out'
            : `Agent exited with code ${agentResult.exitCode}`
        );
      }

      // ── 4. Pull the branch (container pushed changes) ───────────────────────
      const commitStats = await this.verifier.pullBranch(branchName);
      logger.info(`Pulled branch: ${commitStats.filesChanged} files changed`);

      if (commitStats.filesChanged === 0) {
        return this.handleFailure(
          task,
          agent,
          branchName,
          startTime,
          agentResult.output,
          'Agent produced no changes'
        );
      }

      // ── 5. Merge into working tree ──────────────────────────────────────────
      this.updateStatus({ phase: 'merging', task, agent, branch: branchName });
      await this.verifier.mergeToMain(branchName);

      // ── 6. Verify — run CI on host (has node_modules) ──────────────────────
      this.updateStatus({ phase: 'verifying', task, agent, branch: branchName });
      const ciResult = await this.verifier.runCI();

      if (!ciResult.passed) {
        await this.verifier.revertLastMerge();
        return this.handleFailure(
          task,
          agent,
          branchName,
          startTime,
          ciResult.output,
          `CI failed at step: ${ciResult.failedStep}`
        );
      }

      // ── 7. Push + cleanup ──────────────────────────────────────────────────
      if (this.config.pushToRemote) {
        await this.verifier.pushMain();
      }
      await this.verifier.cleanup(branchName);

      // ── 8. Record learning ─────────────────────────────────────────────────
      this.updateStatus({ phase: 'recording', task, agent, branch: branchName });

      const durationMs = Date.now() - startTime;
      const estimatedTokens = agent === 'pi-agent' ? 0 : Math.round(durationMs / 100);

      const result: DogfeedCycleResult = {
        success: true,
        task,
        agent,
        branch: branchName,
        ciPassed: true,
        merged: true,
        durationMs,
        estimatedTokens,
        filesChanged: commitStats.filesChanged,
        linesAdded: commitStats.insertions,
        linesRemoved: commitStats.deletions,
        agentOutput: agentResult.output,
        ciOutput: ciResult.output,
      };

      this.recordResult(result);
      this.analyzer.markDone(
        task,
        'OK',
        Math.round(estimatedTokens / 1000),
        formatDuration(durationMs)
      );

      this.lastResult = result;
      this.updateStatus({ phase: 'completed', task, agent, branch: branchName });

      logger.info(`Cycle ${this.cycleCount} completed successfully: ${task.description}`);
      return result;
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err);
      return this.handleFailure(
        task,
        agent,
        branchName,
        startTime,
        '',
        `Unexpected error: ${error}`
      );
    }
  }

  private async handleFailure(
    task: DogfeedTask,
    agent: 'pi-agent' | 'omc',
    branchName: string,
    startTime: number,
    output: string,
    error: string
  ): Promise<DogfeedCycleResult> {
    logger.error(`Cycle failed: ${error}`);

    try {
      await this.verifier.cleanup(branchName);
    } catch (cleanupErr) {
      logger.warn(`Branch cleanup failed: ${cleanupErr}`);
    }

    const durationMs = Date.now() - startTime;
    const result: DogfeedCycleResult = {
      success: false,
      task,
      agent,
      branch: branchName,
      ciPassed: false,
      merged: false,
      durationMs,
      estimatedTokens: 0,
      filesChanged: 0,
      linesAdded: 0,
      linesRemoved: 0,
      error,
      agentOutput: output,
    };

    this.recordResult(result);
    this.analyzer.markDone(task, `FAILED: ${error}`, 0, formatDuration(durationMs));

    this.lastResult = result;
    this.updateStatus({ phase: 'failed', task, agent, branch: branchName, error });

    return result;
  }

  private recordResult(result: DogfeedCycleResult): void {
    const logPath = this.config.phaseLog;

    if (!fs.existsSync(logPath)) {
      logger.warn(`Phase log not found: ${logPath}`);
      return;
    }

    try {
      let content = fs.readFileSync(logPath, 'utf-8');
      const outcome = result.success ? 'OK' : `FAILED: ${result.error ?? 'unknown'}`;
      const duration = formatDuration(result.durationMs);
      const lines =
        result.linesAdded + result.linesRemoved > 0
          ? `+${result.linesAdded}/-${result.linesRemoved}`
          : '-';

      const row = `| ${this.cycleCount} | ${result.task.description.substring(0, 50)} | ${result.agent} | ${outcome.substring(0, 30)} | ~${result.estimatedTokens} | ${duration} | ${result.filesChanged} | ${lines} | |`;

      const headerLine =
        '|-------|------|-------|---------|--------|----------|-------|-------|-------|';
      const headerIdx = content.indexOf(headerLine);
      if (headerIdx !== -1) {
        const insertAt = content.indexOf('\n', headerIdx) + 1;
        content = content.substring(0, insertAt) + row + '\n' + content.substring(insertAt);
      }

      content = this.updateTotals(content, result);
      fs.writeFileSync(logPath, content, 'utf-8');
    } catch (err) {
      logger.error(`Failed to record result: ${err}`);
    }
  }

  private updateTotals(content: string, result: DogfeedCycleResult): string {
    content = content.replace(/\*\*Total cycles:\*\* \d+/, `**Total cycles:** ${this.cycleCount}`);

    const successMatch = content.match(/\*\*Successful merges:\*\* (\d+)/);
    if (successMatch) {
      const current = parseInt(successMatch[1]!, 10);
      const updated = result.success ? current + 1 : current;
      content = content.replace(
        /\*\*Successful merges:\*\* \d+/,
        `**Successful merges:** ${updated}`
      );
    }

    const failMatch = content.match(/\*\*Failed cycles:\*\* (\d+)/);
    if (failMatch) {
      const current = parseInt(failMatch[1]!, 10);
      const updated = result.success ? current : current + 1;
      content = content.replace(/\*\*Failed cycles:\*\* \d+/, `**Failed cycles:** ${updated}`);
    }

    const tokenMatch = content.match(/\*\*Estimated tokens used:\*\* ([\d,]+)/);
    if (tokenMatch) {
      const current = parseInt(tokenMatch[1]!.replace(/,/g, ''), 10);
      const updated = current + result.estimatedTokens;
      content = content.replace(
        /\*\*Estimated tokens used:\*\* [\d,]+/,
        `**Estimated tokens used:** ${updated.toLocaleString()}`
      );
      content = content.replace(
        /\*\*Budget remaining:\*\* [\d,]+/,
        `**Budget remaining:** ${Math.max(0, 500_000 - updated).toLocaleString()}`
      );
    }

    return content;
  }

  private updateStatus(status: CycleStatus): void {
    this.currentStatus = status;
  }

  private buildEmptyResult(startTime: number): DogfeedCycleResult {
    return {
      success: false,
      task: { priority: -1, category: 'infra', description: 'No tasks available', status: 'ready' },
      agent: 'pi-agent',
      branch: '',
      ciPassed: false,
      merged: false,
      durationMs: Date.now() - startTime,
      estimatedTokens: 0,
      filesChanged: 0,
      linesAdded: 0,
      linesRemoved: 0,
      error: 'No ready tasks available',
    };
  }
}

function formatDuration(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  return `${minutes}m${remainingSeconds}s`;
}
