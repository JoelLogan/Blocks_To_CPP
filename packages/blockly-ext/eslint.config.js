// ESLint flat config (docs/spec/09-quality-and-delivery.md §9.1) with the frontend security
// rules of docs/spec/08-security.md §8.8, the same as apps/desktop's minus the React plugins.
// Keep the two in step. Run: pnpm --filter @blocks2cpp/blockly-ext lint
import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import noUnsanitized from 'eslint-plugin-no-unsanitized';
import tseslint from 'typescript-eslint';

/** Ways to put markup or code into the page from a string. User content is rendered as text. */
const htmlSinks = [
  ['innerHTML', 'Render text as SVG or DOM text (textContent), never as HTML.'],
  ['outerHTML', 'Render text as SVG or DOM text (textContent), never as HTML.'],
  ['insertAdjacentHTML', 'Create elements with the DOM API, never from HTML strings.'],
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
    },
  },

  {
    // Plain JavaScript files (this config) are not part of the TypeScript project, so they get
    // the rules that need no type information.
    files: ['**/*.js'],
    extends: [tseslint.configs.disableTypeChecked],
  },
]);
