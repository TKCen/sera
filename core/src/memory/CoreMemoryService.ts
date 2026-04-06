import type { Pool } from 'pg';

export interface CoreMemoryBlock {
  id: string;
  agentInstanceId: string;
  name: string;
  content: string;
  characterLimit: number;
  isReadOnly: boolean;
  createdAt: string;
  updatedAt: string;
}

export class CoreMemoryService {
  private static instance: CoreMemoryService;

  constructor(private pool: Pool) {}

  static getInstance(pool: Pool): CoreMemoryService {
    if (!CoreMemoryService.instance) {
      CoreMemoryService.instance = new CoreMemoryService(pool);
    }
    return CoreMemoryService.instance;
  }

  async listBlocks(agentInstanceId: string): Promise<CoreMemoryBlock[]> {
    const res = await this.pool.query(
      `SELECT id, agent_instance_id as "agentInstanceId", name, content,
              character_limit as "characterLimit", is_read_only as "isReadOnly",
              created_at as "createdAt", updated_at as "updatedAt"
       FROM core_memory_blocks
       WHERE agent_instance_id = $1
       ORDER BY name ASC`,
      [agentInstanceId]
    );
    return res.rows;
  }

  async getBlock(agentInstanceId: string, name: string): Promise<CoreMemoryBlock | null> {
    const res = await this.pool.query(
      `SELECT id, agent_instance_id as "agentInstanceId", name, content,
              character_limit as "characterLimit", is_read_only as "isReadOnly",
              created_at as "createdAt", updated_at as "updatedAt"
       FROM core_memory_blocks
       WHERE agent_instance_id = $1 AND name = $2`,
      [agentInstanceId, name]
    );
    return res.rows[0] || null;
  }

  async updateBlock(
    agentInstanceId: string,
    name: string,
    updates: { content?: string; characterLimit?: number; isReadOnly?: boolean }
  ): Promise<CoreMemoryBlock> {
    const setClauses: string[] = [];
    const values: any[] = [agentInstanceId, name];
    let idx = 3;

    if (updates.content !== undefined) {
      setClauses.push(`content = $${idx++}`);
      values.push(updates.content);
    }
    if (updates.characterLimit !== undefined) {
      setClauses.push(`character_limit = $${idx++}`);
      values.push(updates.characterLimit);
    }
    if (updates.isReadOnly !== undefined) {
      setClauses.push(`is_read_only = $${idx++}`);
      values.push(updates.isReadOnly);
    }

    if (setClauses.length === 0) {
      const existing = await this.getBlock(agentInstanceId, name);
      if (!existing) throw new Error(`Block ${name} not found`);
      return existing;
    }

    setClauses.push(`updated_at = NOW()`);

    const res = await this.pool.query(
      `UPDATE core_memory_blocks
       SET ${setClauses.join(', ')}
       WHERE agent_instance_id = $1 AND name = $2
       RETURNING id, agent_instance_id as "agentInstanceId", name, content,
                 character_limit as "characterLimit", is_read_only as "isReadOnly",
                 created_at as "createdAt", updated_at as "updatedAt"`,
      values
    );

    if (res.rowCount === 0) {
      throw new Error(`Block ${name} not found`);
    }

    return res.rows[0];
  }

  async appendBlock(
    agentInstanceId: string,
    name: string,
    contentToAppend: string
  ): Promise<CoreMemoryBlock> {
    // Atomic append using a single UPDATE with validation — no read-modify-write race
    const res = await this.pool.query(
      `UPDATE core_memory_blocks
       SET content = RTRIM(content || E'\\n' || $3),
           updated_at = NOW()
       WHERE agent_instance_id = $1 AND name = $2
         AND is_read_only = false
         AND LENGTH(RTRIM(content || E'\\n' || $3)) <= character_limit
       RETURNING id, agent_instance_id as "agentInstanceId", name, content,
                 character_limit as "characterLimit", is_read_only as "isReadOnly",
                 created_at as "createdAt", updated_at as "updatedAt"`,
      [agentInstanceId, name, contentToAppend]
    );

    if (res.rowCount === 0) {
      // Determine the specific error — block not found, read-only, or over limit
      const block = await this.getBlock(agentInstanceId, name);
      if (!block) throw new Error(`Block ${name} not found`);
      if (block.isReadOnly) throw new Error(`Block ${name} is read-only`);
      const wouldBe = (block.content + '\n' + contentToAppend).trim();
      if (wouldBe.length > block.characterLimit) {
        throw new Error(
          `Append failed: Content exceeds character limit of ${block.characterLimit} for block ${name}`
        );
      }
      // Unexpected — retry once (concurrent update may have changed state)
      throw new Error(`Append failed for block ${name} — concurrent modification detected`);
    }

    return res.rows[0] as CoreMemoryBlock;
  }

  async replaceInBlock(
    agentInstanceId: string,
    name: string,
    oldText: string,
    newText: string
  ): Promise<CoreMemoryBlock> {
    // Atomic replace — single UPDATE with validation, no read-modify-write race.
    // The WHERE clause ensures oldText is present AND the result fits the limit.
    const res = await this.pool.query(
      `UPDATE core_memory_blocks
       SET content = REPLACE(content, $3, $4),
           updated_at = NOW()
       WHERE agent_instance_id = $1 AND name = $2
         AND is_read_only = false
         AND content LIKE '%' || $3 || '%'
         AND LENGTH(REPLACE(content, $3, $4)) <= character_limit
       RETURNING id, agent_instance_id as "agentInstanceId", name, content,
                 character_limit as "characterLimit", is_read_only as "isReadOnly",
                 created_at as "createdAt", updated_at as "updatedAt"`,
      [agentInstanceId, name, oldText, newText]
    );

    if (res.rowCount === 0) {
      const block = await this.getBlock(agentInstanceId, name);
      if (!block) throw new Error(`Block ${name} not found`);
      if (block.isReadOnly) throw new Error(`Block ${name} is read-only`);
      if (!block.content.includes(oldText)) {
        throw new Error(`Replace failed: Text "${oldText}" not found in block ${name}`);
      }
      const newContent = block.content.replace(oldText, newText);
      if (newContent.length > block.characterLimit) {
        throw new Error(
          `Replace failed: New content exceeds character limit of ${block.characterLimit} for block ${name}`
        );
      }
      throw new Error(`Replace failed for block ${name} — concurrent modification detected`);
    }

    return res.rows[0] as CoreMemoryBlock;
  }

  async initializeDefaultBlocks(agentInstanceId: string): Promise<void> {
    const defaults = [
      {
        name: 'persona',
        content: 'You are a helpful AI assistant.',
        characterLimit: 2000,
        isReadOnly: false,
      },
      {
        name: 'human',
        content: 'No information about the user yet.',
        characterLimit: 2000,
        isReadOnly: false,
      },
    ];

    for (const block of defaults) {
      await this.pool.query(
        `INSERT INTO core_memory_blocks (agent_instance_id, name, content, character_limit, is_read_only)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (agent_instance_id, name) DO NOTHING`,
        [agentInstanceId, block.name, block.content, block.characterLimit, block.isReadOnly]
      );
    }
  }
}
