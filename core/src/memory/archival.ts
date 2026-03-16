import fs from 'fs/promises';
import path from 'path';
import matter from 'gray-matter';
import type { ArchivalMemory, SearchOptions } from './types.js';

export class ArchivalMemoryStore {
  private memoryPath: string;

  constructor(customPath?: string) {
    // In Docker, it's /app/memory. Locally it's ../../../memory from this file or process.env.MEMORY_PATH
    this.memoryPath = customPath || process.env.MEMORY_PATH || path.join(process.cwd(), '..', 'memory');
    this.ensureDirectory();
  }

  private async ensureDirectory() {
    try {
      await fs.mkdir(this.memoryPath, { recursive: true });
    } catch (err) {
      console.error(`Failed to create memory directory: ${err}`);
    }
  }

  private sanitizeFilename(title: string): string {
    return title.toLowerCase().replace(/[^a-z0-9]/g, '-').replace(/-+/g, '-').replace(/^-|-$/g, '');
  }

  async save(memory: ArchivalMemory): Promise<string> {
    const filename = `${this.sanitizeFilename(memory.title)}.md`;
    const filepath = path.join(this.memoryPath, filename);

    const stringify = (matter as any).stringify || matter;
    const fileContent = stringify(memory.content, memory.metadata);
    await fs.writeFile(filepath, fileContent, 'utf8');
    return filepath;
  }

  async get(title: string): Promise<ArchivalMemory | null> {
    const filename = `${this.sanitizeFilename(title)}.md`;
    const filepath = path.join(this.memoryPath, filename);

    try {
      const content = await fs.readFile(filepath, 'utf8');
      const parse = (matter as any).default || matter;
      const { data, content: markdownBody } = parse(content);

      return {
        title,
        content: markdownBody.trim(),
        metadata: data as any
      };
    } catch (err) {
      return null;
    }
  }

  async search(options: SearchOptions): Promise<ArchivalMemory[]> {
    const files = await fs.readdir(this.memoryPath);
    const results: ArchivalMemory[] = [];

    for (const file of files) {
      if (!file.endsWith('.md')) continue;

      const content = await fs.readFile(path.join(this.memoryPath, file), 'utf8');
      const parse = (matter as any).default || matter;
      const { data, content: markdownBody } = parse(content);
      const memory: ArchivalMemory = {
        title: file.replace('.md', ''),
        content: markdownBody.trim(),
        metadata: data as any
      };

      let matches = true;
      if (options.query) {
        const query = options.query.toLowerCase();
        matches = memory.title.toLowerCase().includes(query) ||
                  memory.content.toLowerCase().includes(query);
      }

      if (matches && options.tags && options.tags.length > 0) {
        matches = options.tags.every(tag => memory.metadata.tags?.includes(tag));
      }

      if (matches) {
        results.push(memory);
      }

      if (options.limit && results.length >= options.limit) break;
    }

    return results;
  }
}
