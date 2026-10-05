/**
 * @blocks2cpp/blockly-ext: everything in the editor that depends on Blockly, behind a small
 * interface (docs/adr/0002-block-editor-blockly.md, docs/spec/02-architecture.md §2.3): block
 * registration from the catalog, custom fields, the Zelos theme, the type-aware connection checker
 * and the variadic mutators.
 *
 * It may depend only on `blockly`, `@blocks2cpp/ipc-types` and `@blocks2cpp/b2c-core-wasm`
 * (tools/check-package-layering.py). User text is rendered only as SVG or DOM text, never as HTML
 * (docs/spec/08-security.md §8.8).
 *
 * Nothing is exported yet; the modules arrive with milestone M2's editor work.
 */
export {};
