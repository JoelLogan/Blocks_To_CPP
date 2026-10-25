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
 * setEditorServices({ symbols, types }); // also connects the checker's oracle and the mutators
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
  setToolboxSymbols,
  toolboxSymbolGroups,
  type DialogService,
  type EditorServices,
  type PassMode,
  type SymbolInfo,
  type SymbolKind,
  type SymbolProvider,
  type ToolboxSymbolSource,
  type TypeOracle,
} from './services';
export * from './shadows';
export * from './text';
export * from './theme';
export * from './generated/catalog';
export * from './mutators';
export * from './checker';
export * from './icons';
// The type rules, signatures and typed slots (docs/plans/m3-contracts/types.md §3.2, slots.md
// §4.2). `TypeClass` and `TypePatternJson` come with the generated catalog, which declares the same
// types.
export {
  applyTypeFn,
  blockSignature,
  checkAccepts,
  ERROR_JSON,
  ERROR_TYPE,
  inClass,
  inputParts,
  isFitError,
  isTypeClass,
  LIB_TYPES,
  libOf,
  libType,
  NO_FACTS,
  parsePatternJson,
  parsePatternText,
  parseTerm,
  parseType,
  prim,
  sameType,
  startTokens,
  staticAssignment,
  renameTermRefs,
  termFromKey,
  termJson,
  termKey,
  toTypeJson,
  TYPE_CLASSES,
  typeBlock,
  typeBlockSig,
  TypeEnv,
  typeFromTerm,
  typeKey,
  typeLabel,
  typeToTerm,
  type Binder,
  type BlockParts,
  type BlockSigTs,
  type BlockTyping,
  type LibTypeInfo,
  type PartType,
  type PrimitiveName,
  type SignatureDef,
  type SlotFit,
  type StaticType,
  type TypeFactsTs,
  type TypeFn,
  type TypePattern,
  type TypeTerm,
  type UserTypeTs,
} from './types';
