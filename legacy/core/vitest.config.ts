import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    exclude: ['**/node_modules/**', '**/dist/**', '**/agent-runtime/**', '**/.claude/worktrees/**'],
    env: {
      SECRETS_MASTER_KEY: 'test-master-key-32-chars-long-exactly!!',
    },
  },
});
