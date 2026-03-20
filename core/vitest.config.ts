import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    exclude: ['**/node_modules/**', '**/dist/**', '**/agent-runtime/dist/**'],
    env: {
      SECRETS_MASTER_KEY: '1111111111111111111111111111111111111111111111111111111111111111',
    },
  },
});
