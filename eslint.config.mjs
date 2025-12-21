// @ts-check
import { defineConfig } from 'eslint-config-hyoban';

export default defineConfig(
  {
    formatting: false,
    lessOpinionated: true,
    preferESM: false,
    react: true,
    tailwindCSS: false,
    ignores: [
      'dist/**',
      'node_modules/**',
      'coverage/**',
      'build/**',
      'packages/**',
      'remixicon/**',
      'plop/**',
    ],
  },
  {
    settings: {
      tailwindcss: {
        whitelist: ['center'],
      },
    },
    rules: {
      'react-refresh/only-export-components': 'off',
      'unicorn/prefer-blob-reading-methods': 'off',
      'unicorn/prefer-math-trunc': 'off',
      '@eslint-react/no-clone-element': 0,
      '@eslint-react/hooks-extra/no-direct-set-state-in-use-effect': 0,
      // NOTE: Disable this temporarily
      'react-compiler/react-compiler': 0,
      'no-restricted-syntax': 0,
      'no-console': 'off',
      '@eslint-react/no-array-index-key': 0,
    },
  },
  {
    files: ['**/*.tsx'],
    rules: {
      '@stylistic/jsx-self-closing-comp': 'error',
    },
  },
);
