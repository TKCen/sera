import pg from 'pg';
import { Logger } from './logger.js';

const logger = new Logger('Database');

const { Pool } = pg;

const pool = new Pool({
  connectionString: process.env.DATABASE_URL,
});

export const query = (text: string, params?: any[]) => pool.query(text, params);

export const initDb = async () => {
  try {
    // Enable pgvector extension
    await query('CREATE EXTENSION IF NOT EXISTS vector');
    
    // Create embeddings table
    await query(`
      CREATE TABLE IF NOT EXISTS embeddings (
        id SERIAL PRIMARY KEY,
        content TEXT NOT NULL,
        metadata JSONB,
        embedding vector(1536), -- 1536 is standard for OpenAI embeddings
        created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
      )
    `);

    // Create IVFFlat index for faster search
    await query(`
      CREATE INDEX IF NOT EXISTS embeddings_vector_idx ON embeddings 
      USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100)
    `);

    // ── Chat Sessions ─────────────────────────────────────────────────────
    await query(`
      CREATE TABLE IF NOT EXISTS chat_sessions (
        id UUID PRIMARY KEY,
        agent_name TEXT NOT NULL,
        title TEXT NOT NULL DEFAULT 'New Chat',
        message_count INT DEFAULT 0,
        created_at TIMESTAMPTZ DEFAULT NOW(),
        updated_at TIMESTAMPTZ DEFAULT NOW()
      )
    `);

    await query(`
      CREATE TABLE IF NOT EXISTS chat_messages (
        id UUID PRIMARY KEY,
        session_id UUID NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
        role TEXT NOT NULL,
        content TEXT NOT NULL,
        metadata JSONB,
        created_at TIMESTAMPTZ DEFAULT NOW()
      )
    `);

    await query(`
      CREATE INDEX IF NOT EXISTS idx_messages_session
      ON chat_messages(session_id, created_at)
    `);

    await query(`
      CREATE INDEX IF NOT EXISTS idx_sessions_agent
      ON chat_sessions(agent_name, updated_at DESC)
    `);

    // ── Agent Instances ───────────────────────────────────────────────────
    await query(`
      CREATE TABLE IF NOT EXISTS agent_instances (
        id UUID PRIMARY KEY,
        template_name TEXT NOT NULL,
        name TEXT NOT NULL,
        workspace_path TEXT NOT NULL,
        container_id TEXT,
        status TEXT DEFAULT 'active',
        created_at TIMESTAMPTZ DEFAULT NOW(),
        updated_at TIMESTAMPTZ DEFAULT NOW()
      )
    `);

    await query(`
      ALTER TABLE chat_sessions ADD COLUMN IF NOT EXISTS agent_instance_id UUID REFERENCES agent_instances(id) ON DELETE SET NULL
    `);

    // ── Token Usage & Quotas (v2 Security Gateway / Epic 14) ──────────────
    await query(`
      CREATE TABLE IF NOT EXISTS token_usage (
        id SERIAL PRIMARY KEY,
        agent_id TEXT NOT NULL,
        circle_id TEXT,
        model TEXT NOT NULL,
        prompt_tokens INT NOT NULL DEFAULT 0,
        completion_tokens INT NOT NULL DEFAULT 0,
        total_tokens INT NOT NULL DEFAULT 0,
        created_at TIMESTAMPTZ DEFAULT NOW()
      )
    `);

    await query(`
      CREATE INDEX IF NOT EXISTS idx_token_usage_agent
      ON token_usage(agent_id, created_at DESC)
    `);

    await query(`
      CREATE TABLE IF NOT EXISTS token_quotas (
        agent_id TEXT PRIMARY KEY,
        max_tokens_per_hour INT NOT NULL DEFAULT 100000,
        max_tokens_per_day INT NOT NULL DEFAULT 1000000,
        updated_at TIMESTAMPTZ DEFAULT NOW()
      )
    `);

    // ── Audit Trail (Epic 18) ─────────────────────────────────────────────
    await query(`
      CREATE TABLE IF NOT EXISTS audit_trail (
        id SERIAL PRIMARY KEY,
        agent_id TEXT NOT NULL,
        action TEXT NOT NULL,
        details JSONB,
        timestamp TIMESTAMPTZ DEFAULT NOW(),
        previous_hash TEXT,
        hash TEXT NOT NULL
      )
    `);

    await query(`
      CREATE INDEX IF NOT EXISTS idx_audit_agent
      ON audit_trail(agent_id, timestamp ASC)
    `);

    logger.info('Database initialized with pgvector, chat sessions, agent instances, token metering, and audit trail');
  } catch (err) {
    logger.error('Database initialization failed:', err);
    throw err;
  }
};
