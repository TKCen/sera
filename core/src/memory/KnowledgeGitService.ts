/**
 * Epic 8 — KnowledgeGitService
 *
 * Manages git-backed knowledge repositories for circles and the global (system)
 * knowledge base. Agents write to their own branch; merging to main requires
 * either operator approval or the merge-without-approval capability.
 */

import path from 'path';
import fs from 'fs/promises';
import { simpleGit } from 'simple-git';
import { query } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import { EmbeddingService } from '../services/embedding.service.js';
import { VectorService } from '../services/vector.service.js';
import type { MemoryNamespace } from '../services/vector.service.js';
import { ScopedMemoryBlockStore } from './blocks/ScopedMemoryBlockStore.js';
import type { KnowledgeBlock, KnowledgeBlockCreateOpts } from './blocks/scoped-types.js';
import { v4 as uuidv4 } from 'uuid';
import matter from 'gray-matter';

const logger = new Logger('KnowledgeGitService');

const KNOWLEDGE_BASE_PATH = process.env.KNOWLEDGE_BASE_PATH ?? '/knowledge';

const SYSTEM_CIRCLE_ID = 'system';

export interface MergeRequest {
  id: string;
  circleId: string;
  agentInstanceId: string;
  agentName: string;
  branch: string;
  status: 'pending' | 'approved' | 'rejected' | 'merged' | 'conflict';
  approvedBy?: string;
  createdAt: string;
  updatedAt: string;
  diffSummary?: string;
}

export interface GitLogEntry {
  commitHash: string;
  authorName: string;
  authorAgentId: string;
  timestamp: string;
  message: string;
}

export class KnowledgeGitService {
  private static instance: KnowledgeGitService;
  private vectorService = new VectorService('_kgs_unused');
  private embeddingService = EmbeddingService.getInstance();

  private constructor() {}

  static getInstance(): KnowledgeGitService {
    if (!KnowledgeGitService.instance) {
      KnowledgeGitService.instance = new KnowledgeGitService();
    }
    return KnowledgeGitService.instance;
  }

  // ── Repo paths ─────────────────────────────────────────────────────────────

  private repoPath(circleId: string): string {
    if (circleId === SYSTEM_CIRCLE_ID) {
      return path.join(KNOWLEDGE_BASE_PATH, 'system');
    }
    return path.join(KNOWLEDGE_BASE_PATH, 'circles', circleId);
  }

  private branchName(agentInstanceId: string): string {
    return `knowledge/agent-${agentInstanceId}`;
  }

  // ── Init ───────────────────────────────────────────────────────────────────

  /** Initialise a circle knowledge repo (idempotent). */
  async initCircleRepo(circleId: string): Promise<void> {
    const repoDir = this.repoPath(circleId);
    await fs.mkdir(repoDir, { recursive: true });

    const git = simpleGit(repoDir);
    const isRepo = await git.checkIsRepo().catch(() => false);
    if (!isRepo) {
      await git.init();
      await git.addConfig('user.email', 'sera-knowledge@sera');
      await git.addConfig('user.name', 'SERA Knowledge');
      // Create initial commit so main branch exists
      const readmePath = path.join(repoDir, 'README.md');
      await fs.writeFile(readmePath, `# Knowledge Base — ${circleId}\n`, 'utf8');
      await git.add('README.md');
      await git.commit(`chore: initialise knowledge repo for ${circleId}`);
      // Ensure the branch is named 'main' regardless of system git config
      await git.branch(['-M', 'main']);
      logger.info(`Initialised knowledge repo for circle "${circleId}" at ${repoDir}`);
    }
  }

  /** Archive a circle knowledge repo (rename to .archived). */
  async archiveCircleRepo(circleId: string): Promise<void> {
    const repoDir = this.repoPath(circleId);
    try {
      await fs.access(repoDir);
      const archivedPath = `${repoDir}.archived-${Date.now()}`;
      await fs.rename(repoDir, archivedPath);
      logger.info(`Archived knowledge repo for circle "${circleId}" to ${archivedPath}`);
    } catch {
      // Directory doesn't exist, nothing to archive
      logger.debug(`No knowledge repo to archive for circle "${circleId}" at ${repoDir}`);
    }
  }

  /** Ensure the system circle repo exists. Called at startup. */
  async initSystemRepo(): Promise<void> {
    await this.initCircleRepo(SYSTEM_CIRCLE_ID);
  }

  // ── Write ──────────────────────────────────────────────────────────────────

  /**
   * Write a knowledge block file and commit to the agent's branch.
   * Returns the block with the resulting commit hash.
   */
  async write(
    circleId: string,
    agentInstanceId: string,
    agentName: string,
    opts: KnowledgeBlockCreateOpts
  ): Promise<{ block: KnowledgeBlock; commitHash: string }> {
    await this.initCircleRepo(circleId);
    const repoDir = this.repoPath(circleId);
    const git = simpleGit(repoDir);

    // Checkout or create the agent's branch (from current main HEAD)
    const branch = this.branchName(agentInstanceId);
    const branches = await git.branchLocal();
    if (!branches.all.includes(branch)) {
      await git.checkoutLocalBranch(branch);
    } else {
      await git.checkout(branch);
    }

    // Build the block
    const block: KnowledgeBlock = {
      id: uuidv4(),
      agentId: agentInstanceId,
      type: opts.type,
      timestamp: new Date().toISOString(),
      tags: opts.tags ?? [],
      importance: opts.importance ?? 3,
      title: opts.title ?? opts.content.slice(0, 80).replace(/\n/g, ' '),
      content: opts.content,
    };

    // Sanitise timestamp for filename
    const ts = block.timestamp.replace(/[:.]/g, '-');
    const typeDir = path.join(repoDir, agentInstanceId, opts.type);
    await fs.mkdir(typeDir, { recursive: true });
    const filePath = path.join(typeDir, `${ts}-${block.id}.md`);
    const frontmatter: Record<string, unknown> = {
      id: block.id,
      agentId: block.agentId,
      type: block.type,
      timestamp: block.timestamp,
      tags: block.tags,
      importance: block.importance,
      title: block.title,
    };
    await fs.writeFile(filePath, matter.stringify(block.content, frontmatter), 'utf8');

    // Commit with agent identity
    const agentEmail = `sera-agent-${opts.agentId ?? agentInstanceId}@${agentInstanceId}`;
    await git.addConfig('user.name', agentName);
    await git.addConfig('user.email', agentEmail);
    await git.add([path.relative(repoDir, filePath)]);
    const result = await git.commit(`knowledge(${opts.type}): ${block.title}`);
    const commitHash = result.commit ?? '';

    // Index into agent-branch Qdrant namespace
    await this.indexBlock(block, circleId, commitHash, filePath, agentInstanceId);

    logger.info(
      `KnowledgeGitService: committed block ${block.id} to ${branch} in circle "${circleId}"`
    );
    return { block, commitHash };
  }

  // ── Merge ──────────────────────────────────────────────────────────────────

  /**
   * Merge the agent's branch to main.
   * If approvedBy is provided (or capability allows it), merge runs immediately
   * and triggers Qdrant re-indexing.
   */
  async mergeToMain(circleId: string, agentInstanceId: string, approvedBy?: string): Promise<void> {
    const repoDir = this.repoPath(circleId);
    const git = simpleGit(repoDir);
    const branch = this.branchName(agentInstanceId);

    await git.checkout('main');
    try {
      await git.merge([branch, '--no-ff', `--message=Merge ${branch} into main`]);
    } catch (err) {
      // Merge conflict — update merge request status
      await git.merge(['--abort']).catch(() => {});
      throw new Error(`Merge conflict on branch ${branch}: ${err}`);
    }

    // Mark any pending merge request as merged
    await query(
      `UPDATE knowledge_merge_requests
         SET status='merged', approved_by=$1, updated_at=now()
       WHERE circle_id=$2 AND agent_instance_id=$3 AND status='pending'`,
      [approvedBy ?? 'system', circleId, agentInstanceId]
    ).catch((_err) => logger.warn('Failed to update merge request status:', _err));

    // Re-index main branch into circle namespace
    const namespace: MemoryNamespace =
      circleId === SYSTEM_CIRCLE_ID ? 'global' : `circle:${circleId}`;
    const store = new ScopedMemoryBlockStore(repoDir);
    await this.vectorService.rebuildNamespace(namespace, repoDir, store, agentInstanceId);

    logger.info(`KnowledgeGitService: merged ${branch} → main in circle "${circleId}"`);
  }

  // ── Auto-merge ─────────────────────────────────────────────────────────────

  /** Called when agent has merge-without-approval capability. */
  async autoMerge(circleId: string, agentInstanceId: string): Promise<void> {
    await this.mergeToMain(circleId, agentInstanceId, 'auto-approved');
  }

  // ── Merge requests ─────────────────────────────────────────────────────────

  async createMergeRequest(
    circleId: string,
    agentInstanceId: string,
    agentName: string
  ): Promise<MergeRequest> {
    const branch = this.branchName(agentInstanceId);
    const diffSummary = await this.diff(circleId, agentInstanceId).catch(() => '');
    const result = await query(
      `INSERT INTO knowledge_merge_requests
         (id, circle_id, agent_instance_id, agent_name, branch, status, diff_summary)
       VALUES ($1,$2,$3,$4,$5,'pending',$6)
       ON CONFLICT (id) DO NOTHING
       RETURNING *`,
      [uuidv4(), circleId, agentInstanceId, agentName, branch, diffSummary]
    );
    const row = result.rows[0] as Record<string, unknown> | undefined;
    if (!row) throw new Error('Failed to create merge request');
    return this.rowToMergeRequest(row);
  }

  async approveMergeRequest(requestId: string, approvedBy: string): Promise<void> {
    const result = await query(
      `UPDATE knowledge_merge_requests
         SET status='approved', approved_by=$1, updated_at=now()
       WHERE id=$2 AND status='pending'
       RETURNING circle_id, agent_instance_id`,
      [approvedBy, requestId]
    );
    const row = result.rows[0] as { circle_id: string; agent_instance_id: string } | undefined;
    if (!row) throw new Error(`Merge request ${requestId} not found or not pending`);
    await this.mergeToMain(row.circle_id, row.agent_instance_id, approvedBy);
  }

  async listMergeRequests(circleId: string): Promise<MergeRequest[]> {
    const result = await query(
      `SELECT * FROM knowledge_merge_requests
       WHERE circle_id=$1
       ORDER BY created_at DESC`,
      [circleId]
    );
    return (result.rows as Record<string, unknown>[]).map((r) => this.rowToMergeRequest(r));
  }

  private rowToMergeRequest(row: Record<string, unknown>): MergeRequest {
    const createdAt = row['created_at'];
    const updatedAt = row['updated_at'];
    return {
      id: row['id'] as string,
      circleId: row['circle_id'] as string,
      agentInstanceId: row['agent_instance_id'] as string,
      agentName: row['agent_name'] as string,
      branch: row['branch'] as string,
      status: row['status'] as MergeRequest['status'],
      ...(row['approved_by'] ? { approvedBy: row['approved_by'] as string } : {}),
      createdAt: createdAt instanceof Date ? createdAt.toISOString() : String(createdAt),
      updatedAt: updatedAt instanceof Date ? updatedAt.toISOString() : String(updatedAt),
      ...(row['diff_summary'] ? { diffSummary: row['diff_summary'] as string } : {}),
    };
  }

  // ── Diff ───────────────────────────────────────────────────────────────────

  async diff(circleId: string, agentInstanceId: string): Promise<string> {
    const repoDir = this.repoPath(circleId);
    const git = simpleGit(repoDir);
    const branch = this.branchName(agentInstanceId);
    try {
      return await git.diff(['main', branch]);
    } catch {
      return '';
    }
  }

  // ── Log ────────────────────────────────────────────────────────────────────

  async log(circleId: string, filePath?: string): Promise<GitLogEntry[]> {
    const repoDir = this.repoPath(circleId);
    const git = simpleGit(repoDir);
    type LogEntry = {
      hash: string;
      author_name: string;
      author_email: string;
      date: string;
      message: string;
    };
    const logResult = await (
      git as unknown as { log: (args: string[]) => Promise<{ all: readonly LogEntry[] }> }
    )
      .log(filePath ? ['main', '--', filePath] : ['main'])
      .catch(() => ({ all: [] as LogEntry[] }));
    return (logResult.all ?? []).map((entry: LogEntry) => ({
      commitHash: entry.hash,
      authorName: entry.author_name,
      authorAgentId: (() => {
        const m = entry.author_email?.match(/sera-agent-([^@]+)@/);
        return m ? m[1]! : (entry.author_email ?? '');
      })(),
      timestamp: entry.date,
      message: entry.message,
    }));
  }

  // ── Indexing ───────────────────────────────────────────────────────────────

  private async indexBlock(
    block: KnowledgeBlock,
    circleId: string,
    commitHash: string,
    sourceFile: string,
    agentInstanceId: string
  ): Promise<void> {
    if (!this.embeddingService.isAvailable()) return;
    try {
      const namespace: MemoryNamespace =
        circleId === SYSTEM_CIRCLE_ID ? 'global' : `circle:${circleId}`;
      const vector = await this.embeddingService.embed(`${block.title}\n${block.content}`);
      await this.vectorService.upsert(block.id, namespace, vector, {
        agent_id: agentInstanceId,
        created_at: block.timestamp,
        tags: block.tags,
        type: block.type,
        title: block.title,
        content: block.content,
        source_file: sourceFile,
        commit_hash: commitHash,
        namespace,
      });
    } catch (err) {
      logger.warn(`KnowledgeGitService: failed to index block ${block.id}:`, err);
    }
  }
}
