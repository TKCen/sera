import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import fs from 'fs';
import path from 'path';
import { SessionStore, type SerializedSession } from '../session.js';

const TEST_SESSION_PATH = './test-session.json';

describe('SessionStore', () => {
  let store: SessionStore;

  beforeEach(() => {
    store = new SessionStore(TEST_SESSION_PATH);
  });

  afterEach(() => {
    if (fs.existsSync(TEST_SESSION_PATH)) {
      fs.unlinkSync(TEST_SESSION_PATH);
    }
  });

  const dummySession: Omit<SerializedSession, 'version' | 'updatedAt'> = {
    agentId: 'test-agent',
    taskId: 'task-123',
    iteration: 2,
    messages: [
      { role: 'user', content: 'hello' },
      { role: 'assistant', content: 'hi', usage: { promptTokens: 10, completionTokens: 5, cacheCreationTokens: 0, cacheReadTokens: 0, totalTokens: 15 } }
    ],
    totalUsage: {
      promptTokens: 10,
      completionTokens: 5,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      totalTokens: 15
    },
    createdAt: new Date().toISOString(),
  };

  it('saves and loads a session', async () => {
    await store.save(dummySession);

    const loaded = await store.load('task-123');
    expect(loaded).not.toBeNull();
    expect(loaded?.taskId).toBe('task-123');
    expect(loaded?.iteration).toBe(2);
    expect(loaded?.messages).toHaveLength(2);
    expect(loaded?.totalUsage.totalTokens).toBe(15);
    expect(loaded?.version).toBe(1);
  });

  it('returns null if taskId mismatch', async () => {
    await store.save(dummySession);
    const loaded = await store.load('different-task');
    expect(loaded).toBeNull();
  });

  it('deletes a session', async () => {
    await store.save(dummySession);
    expect(fs.existsSync(TEST_SESSION_PATH)).toBe(true);

    await store.delete();
    expect(fs.existsSync(TEST_SESSION_PATH)).toBe(false);
  });

  it('cleans up stale sessions', async () => {
    await store.save(dummySession);

    // Mock stat to make it look old
    const oldDate = new Date(Date.now() - 25 * 60 * 60 * 1000); // 25h ago
    vi.spyOn(fs, 'statSync').mockReturnValue({ mtimeMs: oldDate.getTime() } as fs.Stats);

    await store.cleanupStale();
    expect(fs.existsSync(TEST_SESSION_PATH)).toBe(false);

    vi.restoreAllMocks();
  });

  it('does not clean up fresh sessions', async () => {
    await store.save(dummySession);

    // Mock stat to make it look recent
    const recentDate = new Date(Date.now() - 1 * 60 * 60 * 1000); // 1h ago
    vi.spyOn(fs, 'statSync').mockReturnValue({ mtimeMs: recentDate.getTime() } as fs.Stats);

    await store.cleanupStale();
    expect(fs.existsSync(TEST_SESSION_PATH)).toBe(true);

    vi.restoreAllMocks();
  });

  it('handles corrupt session files gracefully', async () => {
    fs.writeFileSync(TEST_SESSION_PATH, 'invalid json');
    const loaded = await store.load('task-123');
    expect(loaded).toBeNull();
  });
});
