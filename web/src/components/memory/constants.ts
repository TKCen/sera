/**
 * Shared constants for memory UI components.
 * Single source of truth — do not duplicate these in other files.
 */

/** Tailwind CSS class map for memory block types (background + text). */
export const MEMORY_TYPE_TAILWIND: Record<string, string> = {
  fact: 'bg-blue-500/15 text-blue-400',
  context: 'bg-purple-500/15 text-purple-400',
  memory: 'bg-green-500/15 text-green-400',
  insight: 'bg-yellow-500/15 text-yellow-400',
  reference: 'bg-cyan-500/15 text-cyan-400',
  observation: 'bg-orange-500/15 text-orange-400',
  decision: 'bg-red-500/15 text-red-400',
};

/** Solid Tailwind background classes for timeline dots. */
export const MEMORY_TYPE_DOT: Record<string, string> = {
  fact: 'bg-blue-500',
  context: 'bg-purple-500',
  memory: 'bg-green-500',
  insight: 'bg-yellow-500',
  reference: 'bg-cyan-500',
  observation: 'bg-orange-500',
  decision: 'bg-red-500',
};

/** Hex color strings for canvas-based rendering (MemoryGraph). */
export const MEMORY_TYPE_HEX: Record<string, string> = {
  // Epic 8 block types
  fact: '#3b82f6',
  context: '#a855f7',
  memory: '#22c55e',
  insight: '#eab308',
  reference: '#06b6d4',
  observation: '#f97316',
  decision: '#ef4444',
  // Legacy types
  human: '#60a5fa',
  persona: '#c084fc',
  core: '#4ade80',
  archive: '#6b7280',
  // Meta-node types (agent/circle)
  agent: '#f472b6',
  circle: '#a78bfa',
};
