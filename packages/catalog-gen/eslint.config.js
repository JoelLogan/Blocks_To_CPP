// ESLint flat config (docs/spec/09-quality-and-delivery.md §9.1) with the security rules of
// docs/spec/08-security.md §8.8, the same as apps/desktop's minus the React plugins (keep them
// in step), plus a ban on starting processes: the generator only reads and writes files.
// Run: pnpm --filter @blocks2cpp/catalog-gen lint
import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import noUnsanitized from 'eslint-plugin-no-unsanitized';
import tseslint from 'typescript-eslint';

/** Ways to put markup or code into a page from a string. */
const htmlSinks = [
  ['innerHTML', 'Generate text, never HTML from strings.'],
  ['outerHTML', 'Generate text, never HTML from strings.'],
  ['insertAdjacentHTML', 'Generate text, never HTML from strings.'],
].map(([property, message]) => ({ property, message }));

export default defineConfig([
  globalIgnores(['coverage/']),

  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,
  {
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },

  noUnsanitized.configs.recommended,

  {
    name: 'blocks2cpp/security',
    rules: {
      'no-eval': 'error',
      'no-new-func': 'error',
      // String arguments to setTimeout/setInterval and the Function constructor.
      '@typescript-eslint/no-implied-eval': 'error',
      'no-script-url': 'error',
      'no-restricted-properties': [
        'error',
        ...htmlSinks,
        { object: 'document', property: 'write', message: 'document.write is not allowed.' },
        { object: 'document', property: 'writeln', message: 'document.writeln is not allowed.' },
      ],
      'no-restricted-syntax': [
        'error',
        {
          selector: "NewExpression[callee.name='Function']",
          message: 'new Function evaluates a string as code.',
        },
      ],
      'no-restricted-imports': [
        'error',
        {
          paths: ['child_process', 'node:child_process', 'worker_threads', 'node:worker_threads'],
        },
      ],
    },
  },

  {
    // Plain JavaScript files (this config) are not part of the TypeScript project, so they get
    // the rules that need no type information.
    files: ['**/*.js'],
    extends: [tseslint.configs.disableTypeChecked],
  },
]);
