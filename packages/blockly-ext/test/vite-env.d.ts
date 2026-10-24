/**
 * The part of Vite's `import.meta.glob` the tests use (Vitest runs on Vite), to read the example
 * projects as text without Node.js file APIs.
 */
interface ImportMeta {
  glob(
    pattern: string,
    options: { readonly query: '?raw'; readonly import: 'default'; readonly eager: true },
  ): Record<string, string>;
}
