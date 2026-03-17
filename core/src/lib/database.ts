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

    logger.info('Database initialized with pgvector and chat sessions');
  } catch (err) {
    logger.error('Database initialization failed:', err);
    throw err;
  }
};
