import fs from 'fs/promises';
import path from 'path';
import matter from 'gray-matter';
import { pool } from '../lib/database.js';
import { MEMORY_BLOCK_TYPES } from '../memory/blocks/types.js';

const MEMORY_ROOT = process.env.MEMORY_PATH ?? path.join(process.cwd(), '..', 'memory');

async function syncMemories() {
  console.log(`Starting sync from ${MEMORY_ROOT} to PostgreSQL...`);

  // 1. Sync global memories
  const globalPath = MEMORY_ROOT;
  await syncDirectory(globalPath, 'global', null);

  // 2. Sync agent memories
  const agentsPath = path.join(MEMORY_ROOT, 'agents');
  if (await exists(agentsPath)) {
    const agents = await fs.readdir(agentsPath);
    for (const agentId of agents) {
      await syncDirectory(path.join(agentsPath, agentId), `personal:${agentId}`, agentId);
    }
  }

  // 3. Sync circle memories
  const circlesPath = path.join(MEMORY_ROOT, 'circles');
  if (await exists(circlesPath)) {
    const circles = await fs.readdir(circlesPath);
    for (const circleId of circles) {
      await syncDirectory(path.join(circlesPath, circleId), `circle:${circleId}`, null);
    }
  }

  console.log('Sync completed.');
  process.exit(0);
}

async function syncDirectory(dirPath: string, logicalNamespace: string, agentId: string | null) {
  for (const type of MEMORY_BLOCK_TYPES) {
    const typeDir = path.join(dirPath, 'blocks', type);
    if (!(await exists(typeDir))) continue;

    const files = await fs.readdir(typeDir);
    for (const file of files) {
      if (!file.endsWith('.md')) continue;
      const filePath = path.join(typeDir, file);
      try {
        const raw = await fs.readFile(filePath, 'utf8');
        const parsed = matter(raw);
        const data = parsed.data;

        if (!data.id) continue;

        await pool.query(
          `INSERT INTO memory_blocks (id, agent_id, namespace, type, title, content, tags, importance, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           ON CONFLICT (id) DO UPDATE SET
             agent_id = EXCLUDED.agent_id,
             namespace = EXCLUDED.namespace,
             type = EXCLUDED.type,
             title = EXCLUDED.title,
             content = EXCLUDED.content,
             tags = EXCLUDED.tags,
             importance = EXCLUDED.importance,
             updated_at = EXCLUDED.updated_at`,
          [
            data.id,
            agentId,
            logicalNamespace,
            data.type || type,
            data.title || '',
            parsed.content.trim(),
            data.tags || [],
            data.importance || 3,
            data.createdAt || new Date(),
            data.updatedAt || new Date(),
          ]
        );
        console.log(`Synced ${data.id} (${logicalNamespace}/${type})`);
      } catch (err) {
        console.error(`Failed to sync ${filePath}:`, err);
      }
    }
  }
}

async function exists(p: string) {
  try {
    await fs.access(p);
    return true;
  } catch {
    return false;
  }
}

syncMemories().catch(console.error);
