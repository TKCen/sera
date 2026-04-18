/**
 * Memory types — re-exports from the blocks system.
 *
 * This module provides a convenience barrel export for memory-related types.
 */
export type {
  MemoryBlockType,
  MemoryEntry,
  MemoryBlock,
  MemoryGraph,
  GraphNode,
  GraphEdge,
  CreateEntryOptions,
  MemorySource,
} from './blocks/types.js';

export { MEMORY_BLOCK_TYPES } from './blocks/types.js';
