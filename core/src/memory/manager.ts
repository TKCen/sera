import { ArchivalMemoryStore } from './archival.js';
import type { ArchivalMemory, WorkingMemory, SearchOptions } from './types.js';

export class MemoryManager {
  private archivalStore: ArchivalMemoryStore;
  private workingMemory: WorkingMemory;

  constructor() {
    this.archivalStore = new ArchivalMemoryStore();
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
