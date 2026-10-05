/**
 * @blocks2cpp/blockly-ext: everything in the editor that depends on Blockly, behind a small
 * interface (docs/adr/0002-block-editor-blockly.md, docs/spec/02-architecture.md §2.3): block
 * registration from the catalog, custom fields, the Zelos theme, the internal expression shadows,
 * the ID generator, the type-aware connection checker and the variadic mutators.
 *
 * It may depend only on `blockly`, `@blocks2cpp/ipc-types` and `@blocks2cpp/b2c-core-wasm`
 * (tools/check-package-layering.py). User text is rendered only as SVG or DOM text, never as HTML
 * (docs/spec/08-security.md §8.8, docs/security/custom-field-review-checklist.md).
 *
 * Typical start-up, before the first workspace is injected:
 *
 * ```ts
 * installIdGenerator();
 * registerB2cMutators();
 * registerB2cBlocks();
 * setEditorServices({ symbols, types, dialogs });
 * Blockly.inject(host, { renderer: 'zelos', theme: b2cLightTheme, … });
 * ```
 */
export * from './blocks';
export * from './fields';
export {
  GENERATED_ID_PATTERN,
  ID_RANDOM_LENGTH,
  IdGeneratorError,
  PROJECT_ID_PATTERN,
  installIdGenerator,
  isIdGeneratorInstalled,
  isProjectId,
  newId,
  type IdKind,
} from './ids';
export {
  DEFAULT_EDITOR_SERVICES,
  getEditorServices,
  resetEditorServices,
  setEditorServices,
  type DialogService,
  type EditorServices,
  type PassMode,
  type SymbolInfo,
  type SymbolKind,
  type SymbolProvider,
  type TypeOracle,
} from './services';
export * from './shadows';
export * from './text';
export * from './theme';
export * from './generated/catalog';
export * from './mutators';
export * from './checker';
export * from './icons';
