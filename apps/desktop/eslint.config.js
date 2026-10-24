// ESLint flat config (docs/spec/09-quality-and-delivery.md §9.1) with the frontend security
// rules of docs/spec/08-security.md §8.8. Run: pnpm --filter @blocks2cpp/desktop lint
import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import jsxA11y from 'eslint-plugin-jsx-a11y';
import noUnsanitized from 'eslint-plugin-no-unsanitized';
import reactHooks from 'eslint-plugin-react-hooks';
import tseslint from 'typescript-eslint';

/** Ways to put markup or code into the page from a string. User content is rendered as text. */
const htmlSinks = [
  ['innerHTML', 'Render text with React (or textContent), never as HTML.'],
  ['outerHTML', 'Render text with React (or textContent), never as HTML.'],
  ['insertAdjacentHTML', 'Create elements with React (or the DOM API), never from HTML strings.'],
].map(([property, message]) => ({ property, message }));

export default defineConfig([
  globalIgnores(['dist/', 'coverage/', 'src-tauri/target/', 'src-tauri/gen/']),

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

  reactHooks.configs['recommended-latest'],
  // Accessibility rules for JSX (docs/spec/04-user-interface.md §4.8: WCAG 2.2 AA). The strict set:
  // with --max-warnings 0 every finding fails the lint anyway.
  { files: ['**/*.{jsx,tsx}'], ...jsxA11y.flatConfigs.strict },
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
          selector: "JSXAttribute[name.name='dangerouslySetInnerHTML']",
          message: 'Render user content as React text, never as HTML.',
        },
        {
          selector: "NewExpression[callee.name='Function']",
          message: 'new Function evaluates a string as code.',
        },
      ],
      'react-hooks/exhaustive-deps': 'error',
    },
  },

  {
    // Plain JavaScript files (this config and the isolation hook) are not part of a
    // TypeScript project, so they get the rules that need no type information.
    files: ['**/*.js'],
    extends: [tseslint.configs.disableTypeChecked],
  },
  {
    files: ['src-tauri/isolation/**/*.js'],
    languageOptions: {
      sourceType: 'script',
      globals: { window: 'readonly' },
    },
  },
]);
