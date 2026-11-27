import js from '@eslint/js';
import prettier from 'eslint-config-prettier';
import reactHooks from 'eslint-plugin-react-hooks';
import { defineConfig, globalIgnores } from 'eslint/config';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import noLiteralUiText from './eslint/no-literal-ui-text.js';

export default defineConfig([
  globalIgnores(['dist', 'src-tauri']),
  {
    files: ['**/*.{ts,tsx,js}'],
    extends: [js.configs.recommended, tseslint.configs.recommended],
    languageOptions: { globals: { ...globals.browser, ...globals.node } },
  },
  {
    files: ['src/**/*.{ts,tsx}'],
    extends: [reactHooks.configs.flat['recommended-latest']],
  },
  {
    // User-facing text comes from the catalogs in locales/ (principle A5).
    files: ['src/**/*.tsx'],
    ignores: ['src/**/*.test.tsx'],
    plugins: { local: { rules: { 'no-literal-ui-text': noLiteralUiText } } },
    rules: { 'local/no-literal-ui-text': 'error' },
  },
  prettier,
]);
