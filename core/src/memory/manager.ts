import path from 'path';
import { ArchivalMemoryStore } from './archival.js';
import type { ArchivalMemory, WorkingMemory, SearchOptions } from './types.js';

export class MemoryManager {
  private archivalStore: ArchivalMemoryStore;
  private workingMemory: WorkingMemory;
  public readonly circleId?: string;

  constructor(circleId?: string) {
    if (circleId !== undefined) {
      this.circleId = circleId;
    }

    // When a circleId is provided, namespace the memory path under circles/{circleId}
    const customPath = circleId
      ? path.join(process.env.MEMORY_PATH || path.join(process.cwd(), '..', 'memory'), 'circles', circleId)
      : undefined;

    this.archivalStore = new ArchivalMemoryStore(customPath);
    this.workingMemory = {
      context: [],
      recentInteractions: []
    };
  }

  // Working Memory methods
  addToWorkingMemory(info: string) {
    this.workingMemory.context.push(info);
    if (this.workingMemory.context.length > 20) {
      const oldest = this.workingMemory.context.shift();
      if (oldest) {
        console.log('Working memory limit reached, consider manual archival or auto-archiving oldest entry.');
      }
    }
  }

  getWorkingMemory(): WorkingMemory {
    return this.workingMemory;
  }

  clearWorkingMemory() {
    this.workingMemory.context = [];
    this.workingMemory.recentInteractions = [];
  }

  // Tiering Logic: Move from Working to Archival
  async archive(title: string, content: string, tags: string[] = []): Promise<string> {
    const memory: ArchivalMemory = {
      title,
      content,
      metadata: {
        tags,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString()
      }
    };

    return await this.archivalStore.save(memory);
  }

  async searchArchival(options: SearchOptions): Promise<ArchivalMemory[]> {
    return await this.archivalStore.search(options);
  }

  async getFromArchival(title: string): Promise<ArchivalMemory | null> {
    return await this.archivalStore.get(title);
  }
}
