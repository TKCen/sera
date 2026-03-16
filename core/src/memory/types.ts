export interface MemoryMetadata {
  tags?: string[];
  createdAt: string;
  updatedAt: string;
  [key: string]: any;
}

export interface ArchivalMemory {
  id?: string;
  title: string;
  content: string;
  metadata: MemoryMetadata;
}

export interface WorkingMemory {
  context: string[];
  recentInteractions: any[];
}

export interface SearchOptions {
  tags?: string[] | undefined;
  query?: string | undefined;
  limit?: number | undefined;
}
