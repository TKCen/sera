import { pool } from '../lib/database.js';

export interface CoreMemoryBlock {
  id: string;
  agentId: string;
  name: string;
  content: string;
  charLimit: number;
  isReadonly: boolean;
  createdAt: string;
  updatedAt: string;
}

export class CoreMemoryService {
  private static instance: CoreMemoryService;

  public static getInstance(): CoreMemoryService {
    if (!CoreMemoryService.instance) {
      CoreMemoryService.instance = new CoreMemoryService();
    }
    return CoreMemoryService.instance;
  }

  /**
   * Fetch all core memory blocks for a given agent.
   */
  async getBlocks(agentId: string): Promise<CoreMemoryBlock[]> {
    const res = await pool.query(
      'SELECT id, agent_id as "agentId", name, content, char_limit as "charLimit", is_readonly as "isReadonly", created_at as "createdAt", updated_at as "updatedAt" FROM agent_core_memory WHERE agent_id = $1 ORDER BY name ASC',
      [agentId]
    );
    return res.rows || [];
  }

  /**
   * Fetch a specific core memory block by name for an agent.
   */
  async getBlockByName(agentId: string, name: string): Promise<CoreMemoryBlock | null> {
    const res = await pool.query(
      'SELECT id, agent_id as "agentId", name, content, char_limit as "charLimit", is_readonly as "isReadonly", created_at as "createdAt", updated_at as "updatedAt" FROM agent_core_memory WHERE agent_id = $1 AND name = $2',
      [agentId, name]
    );
    return res.rows?.[0] || null;
  }

  /**
   * Update a memory block's content.
   */
  async updateBlock(agentId: string, name: string, content: string): Promise<CoreMemoryBlock> {
    const block = await this.getBlockByName(agentId, name);

    if (!block) {
      return this.upsertBlock(agentId, name, content);
    }

    if (block.isReadonly) {
      throw new Error(`Memory block "${name}" is read-only`);
    }

    if (content.length > block.charLimit) {
      throw new Error(`Content exceeds character limit for block "${name}" (${block.charLimit})`);
    }

    const res = await pool.query(
      'UPDATE agent_core_memory SET content = $3, updated_at = NOW() WHERE agent_id = $1 AND name = $2 RETURNING id, agent_id as "agentId", name, content, char_limit as "charLimit", is_readonly as "isReadonly", created_at as "createdAt", updated_at as "updatedAt"',
      [agentId, name, content]
    );

    return res.rows[0];
  }

  /**
   * Upsert a memory block (used for initialization and updates).
   */
  async upsertBlock(
    agentId: string,
    name: string,
    content: string,
    charLimit: number = 2000,
    isReadonly: boolean = false
  ): Promise<CoreMemoryBlock> {
    if (content.length > charLimit) {
      throw new Error(`Content exceeds character limit for block "${name}" (${charLimit})`);
    }

    const res = await pool.query(
      `INSERT INTO agent_core_memory (agent_id, name, content, char_limit, is_readonly, updated_at)
       VALUES ($1, $2, $3, $4, $5, NOW())
       ON CONFLICT (agent_id, name) DO UPDATE
       SET content = EXCLUDED.content,
           char_limit = EXCLUDED.char_limit,
           is_readonly = EXCLUDED.is_readonly,
           updated_at = NOW()
       RETURNING id, agent_id as "agentId", name, content, char_limit as "charLimit", is_readonly as "isReadonly", created_at as "createdAt", updated_at as "updatedAt"`,
      [agentId, name, content, charLimit, isReadonly]
    );

    return res.rows[0];
  }

  /**
   * Initialize default blocks for a new agent instance.
   * Uses ON CONFLICT DO NOTHING to ensure we don't wipe existing content on restart.
   */
  async initializeDefaults(agentId: string): Promise<void> {
    const defaults = [
      { name: 'persona', content: '', limit: 2000, readonly: false },
      { name: 'human', content: '', limit: 2000, readonly: false },
    ];

    for (const d of defaults) {
      await pool.query(
        `INSERT INTO agent_core_memory (agent_id, name, content, char_limit, is_readonly)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (agent_id, name) DO NOTHING`,
        [agentId, d.name, d.content, d.limit, d.readonly]
      );
    }
  }
}
