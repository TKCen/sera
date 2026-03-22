/**
 * Tool Proxy Route — host-side file proxy for dynamically granted paths.
 *
 * Story 3.10: When an agent has a session/one-time filesystem grant for a path
 * outside /workspace, the agent-runtime forwards the tool call here. sera-core
 * validates the grant, executes the file operation on the host filesystem, and
 * returns the result.
 *
 * Endpoints:
 *   POST /v1/tools/proxy — proxied file operation from agent container
 */

import { Router } from 'express';
import type { Request, Response } from 'express';
import fs from 'node:fs';
import path from 'node:path';
import type { IdentityService } from '../auth/IdentityService.js';
import type { AuthService } from '../auth/auth-service.js';
import { createAuthMiddleware } from '../auth/authMiddleware.js';
import type { PermissionRequestService } from '../sandbox/PermissionRequestService.js';
import type { AgentRegistry } from '../agents/registry.service.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('ToolProxy');

/** Allowed tool names for proxy operations. */
type ProxyToolName = 'file-read' | 'file-write' | 'file-list' | 'file-delete';

const ALLOWED_TOOLS: ReadonlySet<string> = new Set<ProxyToolName>([
  'file-read',
  'file-write',
  'file-list',
  'file-delete',
]);

interface ProxyRequestBody {
  tool: string;
  args: Record<string, unknown>;
  grantId?: string;
}

/**
 * Canonicalise a path: resolve relative segments, collapse `..`, and resolve
 * symlinks where the path exists. This is the security boundary — all grant
 * validation uses the canonical path.
 */
function canonicalisePath(rawPath: string): string {
  const resolved = path.resolve(rawPath);
  try {
    return fs.realpathSync(resolved);
  } catch {
    // Path doesn't exist yet (e.g. file-write to a new file) — use resolved
    return resolved;
  }
}

/**
 * Check whether `candidatePath` is covered by a grant for `grantPath`.
 * Both paths must be canonicalised before calling this.
 */
function isPathCoveredByGrant(candidatePath: string, grantPath: string): boolean {
  return candidatePath === grantPath || candidatePath.startsWith(grantPath + path.sep);
}

// ── File Operations ──────────────────────────────────────────────────────────

function proxyFileRead(filePath: string): string {
  if (!fs.existsSync(filePath)) {
    return JSON.stringify({ error: `File not found: ${filePath}` });
  }

  const stat = fs.statSync(filePath);
  if (stat.isDirectory()) {
    return JSON.stringify({ error: `Not a file: ${filePath}` });
  }

  const content = fs.readFileSync(filePath, 'utf-8');
  return JSON.stringify({ result: content });
}

function proxyFileWrite(filePath: string, content: string): string {
  const dir = path.dirname(filePath);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(filePath, content, 'utf-8');
  return JSON.stringify({ result: `File written: ${filePath} (${content.length} bytes)` });
}

function proxyFileList(dirPath: string): string {
  if (!fs.existsSync(dirPath)) {
    return JSON.stringify({ error: `Directory not found: ${dirPath}` });
  }

  const stat = fs.statSync(dirPath);
  if (!stat.isDirectory()) {
    return JSON.stringify({ error: `Not a directory: ${dirPath}` });
  }

  const entries = fs.readdirSync(dirPath, { withFileTypes: true });
  const items = entries.map((e) => {
    const type = e.isDirectory() ? 'dir' : 'file';
    let size = '-';
    if (e.isFile()) {
      try {
        const s = fs.statSync(path.join(dirPath, e.name));
        size = `${s.size}`;
      } catch {
        // ignore stat errors
      }
    }
    return { name: e.name, type, size };
  });

  return JSON.stringify({ result: items });
}

function proxyFileDelete(filePath: string, recursive?: boolean): string {
  if (!fs.existsSync(filePath)) {
    return JSON.stringify({ error: `File not found: ${filePath}` });
  }

  const stat = fs.statSync(filePath);
  if (stat.isDirectory()) {
    const entries = fs.readdirSync(filePath);
    if (entries.length > 0 && !recursive) {
      return JSON.stringify({
        error: `Directory not empty: ${filePath} (use recursive: true)`,
      });
    }
    fs.rmSync(filePath, { recursive: true, force: true });
    return JSON.stringify({ result: `Deleted directory: ${filePath}` });
  }

  fs.unlinkSync(filePath);
  return JSON.stringify({ result: `Deleted file: ${filePath}` });
}

// ── Router Factory ───────────────────────────────────────────────────────────

export function createToolProxyRouter(
  identityService: IdentityService,
  authService: AuthService,
  permissionService: PermissionRequestService,
  registry: AgentRegistry
): Router {
  const router = Router();
  const authMiddleware = createAuthMiddleware(identityService, authService);

  /**
   * POST /v1/tools/proxy
   * Auth: Agent JWT (SERA_IDENTITY_TOKEN)
   * Body: { tool, args, grantId? }
   */
  router.post('/proxy', authMiddleware, async (req: Request, res: Response) => {
    try {
      const identity = req.agentIdentity;
      if (!identity) {
        res.status(401).json({ error: 'Agent authentication required' });
        return;
      }

      const body = req.body as ProxyRequestBody;
      const { tool, args } = body;

      // Validate tool name
      if (!tool || !ALLOWED_TOOLS.has(tool)) {
        res
          .status(400)
          .json({ error: `Invalid tool: ${tool}. Allowed: ${[...ALLOWED_TOOLS].join(', ')}` });
        return;
      }

      // Extract the target path from args
      const rawPath = args['path'] as string | undefined;
      if (!rawPath && tool !== 'file-list') {
        res.status(400).json({ error: 'Missing required arg: path' });
        return;
      }

      const targetPath = canonicalisePath(rawPath ?? '/');
      const agentId = identity.agentId;

      // Check grant validity: session/one-time grants first, then persistent DB grants
      const hasSessionGrant = permissionService.hasActiveGrant(agentId, 'filesystem', targetPath);

      let hasPersistentGrant = false;
      if (!hasSessionGrant) {
        const persistentGrants = await registry.getActiveFilesystemGrants(agentId);
        hasPersistentGrant = persistentGrants.some((g) =>
          isPathCoveredByGrant(targetPath, canonicalisePath(g.value as string))
        );
      }

      if (!hasSessionGrant && !hasPersistentGrant) {
        logger.warn(`Grant denied for agent=${agentId} path=${targetPath}`);
        res.status(403).json({ error: 'grant_not_found' });
        return;
      }

      // Execute the file operation on the host filesystem
      let result: string;
      switch (tool as ProxyToolName) {
        case 'file-read':
          result = proxyFileRead(targetPath);
          break;
        case 'file-write': {
          const content = args['content'] as string | undefined;
          if (content === undefined) {
            res.status(400).json({ error: 'Missing required arg: content' });
            return;
          }
          result = proxyFileWrite(targetPath, content);
          break;
        }
        case 'file-list':
          result = proxyFileList(targetPath);
          break;
        case 'file-delete':
          result = proxyFileDelete(targetPath, args['recursive'] as boolean | undefined);
          break;
      }

      logger.info(`Proxy ${tool} agent=${agentId} path=${targetPath}`);
      res.json(JSON.parse(result));
    } catch (err: unknown) {
      logger.error('Tool proxy error:', err);
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}
