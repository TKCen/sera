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
          patterns: [
            {
              group: ['../agents/*', '!../agents/index.js'],
              message: 'Please import from the barrel export core/src/agents/index.js instead of internal files.',
            },
            {
              group: ['../llm/*', '!../llm/index.js'],
              message: 'Please import from the barrel export core/src/llm/index.js instead of internal files.',
            },
            {
              group: ['../sandbox/*', '!../sandbox/index.js'],
              message: 'Please import from the barrel export core/src/sandbox/index.js instead of internal files.',
            },
            {
              group: ['../memory/*', '!../memory/index.js'],
              message: 'Please import from the barrel export core/src/memory/index.js instead of internal files.',
            },
            {
              group: ['../skills/*', '!../skills/index.js'],
              message: 'Please import from the barrel export core/src/skills/index.js instead of internal files.',
            },
            {
              group: ['../mcp/*', '!../mcp/index.js'],
              message: 'Please import from the barrel export core/src/mcp/index.js instead of internal files.',
            },
            {
              group: ['../auth/*', '!../auth/index.js'],
              message: 'Please import from the barrel export core/src/auth/index.js instead of internal files.',
            },
            {
              group: ['../sessions/*', '!../sessions/index.js'],
              message: 'Please import from the barrel export core/src/sessions/index.js instead of internal files.',
            },
            {
              group: ['../audit/*', '!../audit/index.js'],
              message: 'Please import from the barrel export core/src/audit/index.js instead of internal files.',
            },
          ],
        },
      ],
    },
  },
  {
    files: ['**/*.test.ts', '**/__tests__/**/*.ts'],
    rules: {
      'no-restricted-imports': 'off',
    },
  }
);
