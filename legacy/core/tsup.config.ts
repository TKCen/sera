import { defineConfig } from 'tsup';

export default defineConfig({
  entry: ['src/**/*.ts', '!src/**/*.test.ts', '!src/**/__tests__/**'],
  format: ['esm'],
  target: 'esnext',
  platform: 'node',
  outDir: 'dist',
  bundle: false, // file-per-file transpile — preserves dist/ mirror of src/
  sourcemap: true,
  clean: true,
  dts: false, // core is an app, not a library — skip declaration emit
});
