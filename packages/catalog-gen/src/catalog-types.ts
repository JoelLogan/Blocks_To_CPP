// The types of the editor's block definitions and toolbox (docs/spec/03-block-language.md
// §3.11.1). The generator copies this file into packages/blockly-ext/src/generated/catalog.ts, so
// the generated module is self-contained; it must not import anything.

/**
 * A toolbox category, in toolbox order (spec §3.7). Milestone M3's wave W2 adds `collections`,
 * `types`, `files` and `errors` (docs/plans/m3-contracts/w2.md §2.1); a category may have no
 * entries yet, and the editor does not show it then.
 */
export type CategoryId =
  | 'program'
  | 'variables'
  | 'math'
  | 'logic'
  | 'text'
  | 'control'
  | 'loops'
  | 'io'
  | 'functions'
  | 'collections'
  | 'types'
  | 'files'
  | 'errors';

/** A block's shape (spec §3.3). */
export type Shape = 'hat' | 'definition' | 'statement' | 'reporter' | 'predicate';

/**
 * A class of types (`b2c_ir::TypeClass`, docs/plans/m3-contracts/types.md §1.3), as catalog files
 * write it: what a value input's `check` accepts and what a signature's `typeVars` allow a type
 * variable to be.
 */
export type TypeClass =
  | 'any'
  | 'number'
  | 'integer'
  | 'bool'
  | 'text'
  | 'value'
  | 'collection'
  | 'sequence'
  | 'indexable'
  | 'insertable'
  | 'searchable'
  | 'iterable'
  | 'keyed'
  | 'adaptor'
  | 'listable'
  | 'sortable'
  | 'switchable'
  | 'comparable'
  | 'printable'
  | 'struct'
  | 'enum'
  | 'input_file'
  | 'output_file'
  | 'exception';

/**
 * The kind of a field. `header` (types.md §5.4) holds a standard header from the header table as
 * its text, such as `<cmath>`.
 */
export type FieldKind =
  'dropdown' | 'checkbox' | 'text' | 'number' | 'type' | 'symbol_decl' | 'symbol_ref' | 'header';

/**
 * The type of the value a reporter or predicate gives: its pattern as catalog files write it
 * (types.md §4.2), in C++ syntax. M2's values keep their meaning: a type (`bool`, `int`, `double`,
 * `char`, `std::string`), `any` and `number` (the analyser decides), `symbol` (the type of the
 * symbol its `symbol_ref` field refers to) and `field:NAME` (the type chosen in its `type` field
 * `NAME`); milestone M3 adds variables and type functions (`T`, `elem(C)`) and `symbol:NAME`.
 */
export type OutputType = string;

/**
 * A type as files and the wire write it (types.md §3.1): a name, `{"ref": id}`, or a template
 * object with one key, its qualified C++ name, whose value is its argument or an array of two or
 * more.
 */
export type TypeJson = string | { readonly [name: string]: TypeJson | readonly TypeJson[] };

/**
 * A type pattern on a part (types.md §4.2, §4.5), tagged by `kind`: `any` and `number` (M2), a fixed
 * type, a type variable, a library template over patterns, a type function, and the forms that take
 * a part's type (`field:`, `symbol`, `param:`, `member:`, `result`).
 */
export type TypePatternJson =
  | { readonly kind: 'any' }
  | { readonly kind: 'number' }
  | { readonly kind: 'type'; readonly type: TypeJson }
  | { readonly kind: 'var'; readonly name: string }
  | { readonly kind: 'lib'; readonly name: string; readonly args: readonly TypePatternJson[] }
  | { readonly kind: 'fn'; readonly fn: 'elem' | 'index' | 'key'; readonly of: TypePatternJson }
  | { readonly kind: 'field'; readonly field: string }
  | { readonly kind: 'symbol'; readonly field: string | null }
  | { readonly kind: 'param'; readonly field: string }
  | { readonly kind: 'member'; readonly field: string }
  | { readonly kind: 'result' };

/** A part of a block that has a pattern, in catalog order: the order it binds in. */
export type PartNameJson = { readonly field: string } | { readonly input: string };

/** One entry of the type table (types.md §1.1): a fundamental type, `std::string` or a library type. */
export interface TypeEntryJson {
  /** Its JSON name: `int`, `std::string`, `std::vector`. */
  readonly name: string;
  /** How many template arguments it takes (0 for a type written as a name). */
  readonly arity: number;
  /** Its friendly label, with `%1` and `%2` for the arguments' labels (`list of %1`). */
  readonly label: string;
  /** The standard header that declares it, if any. */
  readonly header: string | null;
}

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

/**
 * Repetition: parts `NAME0` to `NAME{count + plus - 1}`, where `count` is an `extra` key: a count,
 * or a row extra (`params`, `fields`), whose number of rows is the count (w2.md §2.1).
 */
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
  /**
   * The types a `type` field offers first, as the field's values: a name stays itself, any other
   * type is its term's key (canonical compact JSON, types.md §3.2).
   */
  readonly types: readonly string[];
  /** The default value; a field without one is required. */
  readonly default: string | boolean | null;
  /** The field's pattern, on `type`, `symbol_ref` and `symbol_decl` fields; `null` for none. */
  readonly type: TypePatternJson | null;
  /**
   * A repeated field (w2.md §2.1): parts `NAME0`, `NAME1`, …, each with the field's default, drawn
   * by the block's mutator as members of its repeated group; `null` for a field shown once.
   */
  readonly repeat: RepeatJson | null;
  /**
   * Present and `true` when the editor writes the field into a project only while it holds
   * something other than its default, so that a block left at the default reads as before the
   * field existed (every `SHARED`, w3-modules.md §6 W3M-D11). A missing field reads as its default.
   */
  readonly omitDefault?: true;
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
  /** The input's pattern, for each part of a repeated input; `null` for none. */
  readonly type: TypePatternJson | null;
  /** `assign = true`: the value is assigned, not used to initialise (`char` fits `std::string`). */
  readonly assign: boolean;
}

/** A statement-input definition. */
export interface StatementDefJson {
  readonly name: string;
  readonly repeat: RepeatJson | null;
  /** The input exists only while this `extra` flag is true. */
  readonly when: string | null;
}

/**
 * The kinds of row extras (w2.md §2.1): arrays of rows, each declaring a symbol. `params` rows are
 * `{sym, name, type, mode}` (a function's parameters), `fields` rows `{sym, name, type}` (a
 * struct's fields) and `values` rows `{sym, name}` (an enum's values). A row extra has the name of
 * its kind.
 */
export type RowExtraKind = 'params' | 'fields' | 'values';

/** A mutator-state (`extra`) key definition. */
export interface ExtraDefJson {
  readonly name: string;
  readonly kind: 'count' | 'flag' | RowExtraKind;
  /** The smallest count (counts only). */
  readonly min: number | null;
  /** The largest count, or the most rows (counts and row extras). */
  readonly max: number | null;
  /** A count's or a flag's default. */
  readonly default: number | boolean | null;
  /**
   * The types a row offers (`params` and `fields` rows), as type field values (see
   * {@link FieldDefJson.types}).
   */
  readonly types: readonly string[];
  /**
   * The class of types a row accepts besides its `types` (`params` and `fields` rows), or `null`.
   */
  readonly typeClass: TypeClass | null;
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
  /** Each type variable and its class (`where`, types.md §4.1). */
  readonly typeVars: Readonly<Record<string, TypeClass>>;
  /** The parts that have a pattern, in catalog order: the order they bind in. */
  readonly order: readonly PartNameJson[];
  /** The output's variable when no part binds it: the block takes its type from its place. */
  readonly placeVar: string | null;
  /** `symbol_ref` fields that list the members of the user type bound to a variable. */
  readonly members: Readonly<Record<string, string>>;
  /** The standard headers every form of the block needs (`<iostream>`). */
  readonly headers: readonly string[];
  /** The headers a dropdown option adds, by field, then option. */
  readonly optionHeaders: Readonly<Record<string, Readonly<Record<string, readonly string[]>>>>;
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
