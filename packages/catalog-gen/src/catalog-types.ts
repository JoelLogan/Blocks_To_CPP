// The types of the editor's block definitions and toolbox (docs/spec/03-block-language.md
// §3.11.1). The generator copies this file into packages/blockly-ext/src/generated/catalog.ts, so
// the generated module is self-contained; it must not import anything.

/** A toolbox category, in toolbox order (spec §3.7). */
export type CategoryId =
  'program' | 'variables' | 'math' | 'logic' | 'text' | 'control' | 'loops' | 'io' | 'functions';

/** A block's shape (spec §3.3). */
export type Shape = 'hat' | 'definition' | 'statement' | 'reporter' | 'predicate';

/** The type class of a value input, for the connection checker (spec §3.5.3). */
export type TypeClass = 'any' | 'number' | 'integer' | 'bool' | 'text';

/** The kind of a field. */
export type FieldKind =
  'dropdown' | 'checkbox' | 'text' | 'number' | 'type' | 'symbol_decl' | 'symbol_ref';

/**
 * The type of the value a reporter or predicate gives. `symbol` is the type of the symbol its
 * `symbol_ref` field refers to (from the live analysis); `field:NAME` is the type chosen in its
 * `type` field `NAME`.
 */
export type OutputType =
  'any' | 'bool' | 'int' | 'double' | 'number' | 'char' | 'string' | 'symbol' | `field:${string}`;

/**
 * One part of a friendly label: plain text, the field or input `arg` (from `%NAME`), or the place
 * where a repeated group continues (from a bare `…`).
 */
export type LabelPart =
  { readonly text: string } | { readonly arg: string } | { readonly repeat: true };

/** An expression token, as in project files (spec §5.5). */
export type TokenJson =
  | { readonly num: string }
  | { readonly str: string }
  | { readonly chr: string }
  | { readonly ref: string }
  | { readonly op: string }
  | { readonly kw: string }
  | { readonly text: string };

/** Repetition: parts `NAME0` to `NAME{count + plus - 1}`, where `count` is an `extra` key. */
export interface RepeatJson {
  readonly count: string;
  readonly plus: number;
}

/** A field definition. */
export interface FieldDefJson {
  readonly name: string;
  readonly kind: FieldKind;
  /** Dropdown options as [label, value] pairs. */
  readonly options: readonly (readonly [label: string, value: string])[];
  /** The type names a `type` field offers. */
  readonly types: readonly string[];
  /** The default value; a field without one is required. */
  readonly default: string | boolean | null;
}

/** A value-input definition. */
export interface InputDefJson {
  readonly name: string;
  readonly check: TypeClass;
  /** The input may stay empty. */
  readonly optional: boolean;
  readonly repeat: RepeatJson | null;
  /** The tokens shown (as a shadow) while the input is empty; none for no default. */
  readonly default: readonly TokenJson[];
}

/** A statement-input definition. */
export interface StatementDefJson {
  readonly name: string;
  readonly repeat: RepeatJson | null;
  /** The input exists only while this `extra` flag is true. */
  readonly when: string | null;
}

/** A mutator-state (`extra`) key definition. */
export interface ExtraDefJson {
  readonly name: string;
  readonly kind: 'count' | 'flag' | 'params';
  /** The smallest count (counts only). */
  readonly min: number | null;
  /** The largest count, or the most parameter rows (counts and params). */
  readonly max: number | null;
  /** A count's or a flag's default. */
  readonly default: number | boolean | null;
  /** The parameter types (params only). */
  readonly types: readonly string[];
}

/** A block definition. */
export interface BlockDefJson {
  readonly id: string;
  readonly version: number;
  readonly category: CategoryId;
  readonly shape: Shape;
  /** Label templates: `%NAME` marks a field or input, a bare `…` a repeated group. */
  readonly label: { readonly friendly: string; readonly cpp: string };
  /** The friendly label, parsed. */
  readonly labelParts: readonly LabelPart[];
  /** Statement inputs the friendly label does not mention, in catalog order: shown after it. */
  readonly statementsNotInLabel: readonly string[];
  /** One-line help, shown as the tooltip (plain text). */
  readonly help: string;
  readonly output: OutputType | null;
  readonly fields: readonly FieldDefJson[];
  readonly inputs: readonly InputDefJson[];
  readonly statements: readonly StatementDefJson[];
  readonly extra: readonly ExtraDefJson[];
}

/** Blocks the editor adds to a category for the open project. */
export type DynamicCategory = 'variables' | 'functions';

/** Values a toolbox entry gives its block instead of the catalog defaults. */
export interface PresetJson {
  readonly fields?: Readonly<Record<string, unknown>>;
  readonly extra?: Readonly<Record<string, unknown>>;
  /** Input tokens by input name (a numbered name for a repeated input). */
  readonly inputs?: Readonly<Record<string, readonly TokenJson[]>>;
}

/** One block in a toolbox category. */
export interface ToolboxEntryJson {
  readonly block: string;
  /** A label shown with the block. */
  readonly label: string | null;
  readonly preset: PresetJson | null;
}

/** A toolbox category. */
export interface ToolboxCategoryJson {
  readonly id: CategoryId;
  readonly name: string;
  /** A short icon shown with the name, so colour is never the only cue. */
  readonly icon: string;
  /** The colour token (the category ID); the editor theme holds the colour values. */
  readonly colour: string;
  readonly dynamic: DynamicCategory | null;
  readonly entries: readonly ToolboxEntryJson[];
}
