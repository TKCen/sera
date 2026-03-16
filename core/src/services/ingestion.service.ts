import fs from 'fs/promises';
import path from 'path';
import { EmbeddingService } from './embedding.service.js';
import { VectorService } from './vector.service.js';
import { v4 as uuidv4 } from 'uuid';

export class IngestionService {
  private workspaceRoot = '/app/workspace';
  private embeddingService = EmbeddingService.getInstance();
  private vectorService = new VectorService();

  constructor() {}

  public async ingestCodebase() {
    console.log('Starting codebase ingestion...');
    const files = await this.recursiveReadDir(this.workspaceRoot);

    // Ensure collection exists (MiniLM-L6-v2 produces 384 dimensional vectors)
    await this.vectorService.ensureCollection(384);

    for (const file of files) {
      if (this.shouldIgnore(file)) continue;

      try {
        const content = await fs.readFile(file, 'utf-8');
        const chunks = this.chunkText(content);

        console.log(`Processing ${file} (${chunks.length} chunks)`);

        const points = [];
        for (let i = 0; i < chunks.length; i++) {
          const chunk = chunks[i];
          if (!chunk) continue;
          const embedding = await this.embeddingService.generateEmbedding(chunk);

          points.push({
            id: uuidv4(),
            vector: embedding,
            payload: {
              path: path.relative(this.workspaceRoot || '', file),
              content: chunk,
              chunkIndex: i,
            } as Record<string, any>,
          });
        }

        if (points.length > 0) {
          await this.vectorService.upsertPoints(points);
        }
      } catch (error) {
        console.error(`Error processing file ${file}:`, error);
      }
    }
    console.log('Codebase ingestion complete.');
  }

  private async recursiveReadDir(dir: string): Promise<string[]> {
    const dirents = await fs.readdir(dir, { withFileTypes: true });
    const files = await Promise.all(
      dirents.map((dirent) => {
        const res = path.resolve(dir, dirent.name);
        return dirent.isDirectory() ? this.recursiveReadDir(res) : res;
      })
    );
    return files.flat();
  }

  private shouldIgnore(filePath: string): boolean {
    const ignoreList = [
      'node_modules', 
      '.git', 
      'dist', 
      '.next', 
      'qdrant_data', 
      'sera_db_data', 
      'certs', 
      'logs', 
      '.pem', 
      '.key', 
      '.crt',
      'package-lock.json',
      'pnpm-lock.yaml',
      'yarn.lock'
    ];
    return ignoreList.some((pattern) => filePath.includes(pattern));
  }

  private chunkText(text: string, chunkSize: number = 1000, overlap: number = 200): string[] {
    const chunks: string[] = [];
    let start = 0;

    while (start < text.length) {
      const end = Math.min(start + chunkSize, text.length);
      chunks.push(text.slice(start, end));
      start += chunkSize - overlap;
    }

    return chunks;
  }
}
