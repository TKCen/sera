import js from '@eslint/js';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  { ignores: ['dist', 'node_modules'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,mts}'],
    languageOptions: {
      ecmaVersion: 2022,
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/no-unused-vars': ['warn', { argsIgnorePattern: '^_' }],
      'no-restricted-imports': [
        'error',
        {
          paths: [
            {
              name: './agents/Orchestrator.js',
              message: 'Please import from "./agents/index.js" instead.',
            },
            {
              name: '../agents/Orchestrator.js',
              message: 'Please import from "../agents/index.js" instead.',
            },
            {
              name: '../../agents/Orchestrator.js',
              message: 'Please import from "../../agents/index.js" instead.',
            },
          ],
          patterns: [
            {
              group: ['**/agents/*', '!**/agents/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
            {
              group: ['**/llm/*', '!**/llm/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
            {
              group: ['**/sandbox/*', '!**/sandbox/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
            {
              group: ['**/memory/*', '!**/memory/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
            {
              group: ['**/skills/*', '!**/skills/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
            {
              group: ['**/mcp/*', '!**/mcp/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
            {
              group: ['**/auth/*', '!**/auth/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
            {
              group: ['**/sessions/*', '!**/sessions/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
            {
              group: ['**/audit/*', '!**/audit/index.js'],
              message: 'Please import from the barrel "index.js" instead of internal files.',
            },
          ],
        },
      ],
    },
  },
  {
    files: ['src/agents/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  {
    files: ['src/llm/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  {
    files: ['src/sandbox/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  {
    files: ['src/memory/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  {
    files: ['src/skills/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  {
    files: ['src/mcp/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  {
    files: ['src/auth/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  {
    files: ['src/sessions/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  },
  {
    files: ['src/audit/**/*'],
    rules: {
      'no-restricted-imports': 'off',
    },
  }
);
