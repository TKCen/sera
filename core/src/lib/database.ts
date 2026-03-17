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

    logger.info('Database initialized with pgvector');
  } catch (err) {
    logger.error('Database initialization failed:', err);
    throw err;
  }
};
