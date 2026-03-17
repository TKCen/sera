import type { LLMProvider } from '../lib/llm/types.js';
import type { MemoryManager } from './manager.js';
import type { MemoryEntry } from './blocks/types.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('Reflector');

/**
 * Reflector — auto-compaction service for agent memory.
 *
 * When the `core` block exceeds a configurable entry threshold, the Reflector
 * summarises the oldest entries via an LLM, creates a new `archive` summary
 * that refs back to the originals, and moves the originals to `archive`.
 */
export class Reflector {
  /** Default: compact when core has more than 20 entries. */
  static readonly DEFAULT_THRESHOLD = 20;

  /** Number of oldest entries to summarise per compaction cycle. */
  static readonly DEFAULT_BATCH_SIZE = 5;

  /** Configurable override for threshold. */
  static compactionThreshold: number | undefined;

  /**
   * Run a compaction cycle if the core block exceeds the threshold.
   * Returns the summary entry if compaction occurred, or null otherwise.
   */
  static async compactIfNeeded(
    memoryManager: MemoryManager,
    llmProvider: LLMProvider,
    opts?: { threshold?: number; batchSize?: number },
  ): Promise<MemoryEntry | null> {
    const threshold = opts?.threshold ?? Reflector.compactionThreshold ?? Reflector.DEFAULT_THRESHOLD;
    const batchSize = opts?.batchSize ?? Reflector.DEFAULT_BATCH_SIZE;

    const coreBlock = await memoryManager.getBlock('core');

    if (coreBlock.entries.length <= threshold) {
      return null;
    }

    // Select the oldest entries for compaction (already sorted oldest-first by store)
    const toCompact = coreBlock.entries.slice(0, batchSize);

    // Build the summarisation prompt
    const contentForSummary = toCompact
      .map(e => `## ${e.title}\n${e.content}`)
      .join('\n\n---\n\n');

    let summaryContent = '';
    try {
      const response = await llmProvider.chat([
        {
          role: 'system',
          content:
            'You are a memory compaction assistant. Summarise the following knowledge entries ' +
            'into a single concise summary paragraph that preserves all key facts and decisions. ' +
            'Do NOT add any preamble or explanation — output only the summary.',
        },
        { role: 'user', content: contentForSummary },
      ]);
      summaryContent = response.content.trim();
    } catch (err) {
      logger.warn(`First summarisation attempt failed. Retrying...`, err);
      try {
        const retryResponse = await llmProvider.chat([
          {
            role: 'system',
            content:
              'You are a memory compaction assistant. Summarise the following knowledge entries ' +
              'into a single concise summary paragraph that preserves all key facts and decisions. ' +
              'Do NOT add any preamble or explanation — output only the summary.',
          },
          { role: 'user', content: contentForSummary },
        ]);
        summaryContent = retryResponse.content.trim();
      } catch (retryErr) {
        logger.error(`Summarisation failed after retry. Skipping compaction.`, retryErr);
        return null;
      }
    }

    // Collect the IDs of compacted entries for the ref chain
    const compactedIds = toCompact.map(e => e.id);

    // Create the summary entry in the archive block, referencing originals
    const summaryTitle = `Summary — ${toCompact.map(e => e.title).join(', ')}`;
    const summaryEntry = await memoryManager.addEntry('archive', {
      title: summaryTitle.length > 120 ? summaryTitle.slice(0, 117) + '...' : summaryTitle,
      content: summaryContent,
      refs: compactedIds,
      tags: ['compaction', 'reflector'],
      source: 'reflector',
    });

    // Move the original entries to archive (preserves their IDs and refs)
    for (const entry of toCompact) {
      await memoryManager.archiveEntry(entry.id);
    }

    logger.info(
      `Compacted ${toCompact.length} core entries into 1 archive summary (ID: ${summaryEntry.id}). Title: "${summaryEntry.title}"`
    );

    return summaryEntry;
  }
}
