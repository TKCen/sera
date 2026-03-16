import fs from 'fs/promises';
import path from 'path';
import { query } from './database.js';

const EMBD_MODEL = process.env.EMBEDDING_MODEL || 'local-model';
const LMSTUDIO_URL = process.env.LMSTUDIO_URL || 'http://host.docker.internal:1234/v1';

export class IngestionService {
  private workspacePath: string;

  constructor(workspacePath: string = '/app/workspace') {
    this.workspacePath = workspacePath;
  }

  async scan() {
    console.log(`🔍 Scanning workspace: ${this.workspacePath}`);
    const files = await this.getFiles(this.workspacePath);
    console.log(`📄 Found ${files.length} files to process`);
    
    for (const file of files) {
      await this.processFile(file);
    }
  }

  private async getFiles(dir: string): Promise<string[]> {
    const entries = await fs.readdir(dir, { withFileTypes: true });
    const files = await Promise.all(entries.map((res) => {
      const resPath = path.resolve(dir, res.name);
      if (res.isDirectory()) {
        // Skip node_modules, .git, etc.
        if (['node_modules', '.git', 'dist', '.next'].includes(res.name)) return [];
        return this.getFiles(resPath);
      }
      return resPath;
    }));
    return files.flat();
  }

  private async processFile(filePath: string) {
    const ext = path.extname(filePath);
    const supportedExts = ['.ts', '.js', '.tsx', '.jsx', '.md', '.json'];
    
    if (!supportedExts.includes(ext)) return;

    try {
      const content = await fs.readFile(filePath, 'utf-8');
      const relativePath = path.relative(this.workspacePath, filePath);
      
      console.log(`⚙️ Processing: ${relativePath}`);
      
      // Basic chunking (to be improved)
      const chunks = this.chunkText(content);
      
      for (let i = 0; i < chunks.length; i++) {
        const chunk = chunks[i];
        const embedding = await this.generateEmbedding(chunk);
        
        if (embedding) {
          await this.storeChunk(chunk, embedding, { 
            path: relativePath, 
            chunkIndex: i,
            extension: ext
          });
        }
      }
    } catch (err) {
      console.error(`❌ Failed to process ${filePath}:`, err);
    }
  }

  private async generateEmbedding(text: string): Promise<number[] | null> {
    try {
      const response = await fetch(`${LMSTUDIO_URL}/embeddings`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          input: text,
          model: EMBD_MODEL
        })
      });

      const data = await response.json();
      return data.data[0].embedding;
    } catch (err) {
      console.error('❌ Embedding generation failed:', err);
      return null;
    }
  }

  private async storeChunk(content: string, embedding: number[], metadata: any) {
    try {
      await query(
        'INSERT INTO embeddings (content, embedding, metadata) VALUES ($1, $2, $3)',
        [content, JSON.stringify(embedding), JSON.stringify(metadata)]
      );
    } catch (err) {
      console.error('❌ Failed to store chunk:', err);
    }
  }

  private chunkText(text: string, size: number = 1000): string[] {
    const chunks: string[] = [];
    let start = 0;
    while (start < text.length) {
      chunks.push(text.slice(start, start + size));
      start += size;
    }
    return chunks;
  }
}
