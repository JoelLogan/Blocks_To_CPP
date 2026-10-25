// Turns the Rust (serde) shapes of catalog.json into the editor's shapes (catalog-types.ts):
// plural, camelCase names (`field` becomes `fields`), numbers for count defaults (`"1"` becomes
// 1), null or empty values for keys that do not apply to a kind or that the export leaves out
// (a block's signature keys, types.md §4.5), the entries of `types` lists as type field values
// (their keys, types.md §3.2), and parsed friendly labels.

import type {
  BlockDefJson,
  ExtraDefJson,
  FieldDefJson,
  InputDefJson,
  OutputType,
  PartNameJson,
  PresetJson,
  RepeatJson,
  StatementDefJson,
  TokenJson,
  ToolboxCategoryJson,
  ToolboxEntryJson,
  TypeEntryJson,
} from './catalog-types.ts';
import {
  CatalogJsonError,
  type CatalogJson,
  type RustBlockDef,
  type RustExtraDef,
  type RustPartName,
  type RustPreset,
  type RustRepeat,
  type RustToken,
  type RustToolboxCategory,
  type RustTypeJson,
} from './catalog-json.ts';
import { labelArgs, parseLabel } from './labels.ts';

/** Everything the generated module exports. */
export interface Model {
  readonly catalogVersion: string;
  readonly blocks: readonly BlockDefJson[];
  readonly toolbox: readonly ToolboxCategoryJson[];
  /** The type table (types.md §1.1); empty when the export has none. */
  readonly types: readonly TypeEntryJson[];
}

/** Converts the export into the editor's shapes. Block order (by ID) is kept. */
export function normalize(input: CatalogJson): Model {
  return {
    catalogVersion: input.catalogVersion,
    blocks: input.blocks.map((block, index) => blockDef(block, `blocks[${String(index)}]`)),
    toolbox: input.toolbox.category.map(category),
    types: (input.types ?? []).map((entry) => ({ ...entry })),
  };
}

function blockDef(block: RustBlockDef, path: string): BlockDefJson {
  const labelParts = parseLabel(block.label.friendly);
  const args = new Set(labelArgs(labelParts));
  return {
    id: block.id,
    version: block.version,
    category: block.category,
    shape: block.shape,
    label: { friendly: block.label.friendly, cpp: block.label.cpp },
    labelParts,
    statementsNotInLabel: block.statement.map((s) => s.name).filter((name) => !args.has(name)),
    help: block.help,
    output: output(block.output, `${path}.output`),
    fields: block.field.map((field): FieldDefJson => ({
      name: field.name,
      kind: field.kind,
      options: field.options.map(([label, value]) => [label, value] as const),
      types: field.types.map(typeKey),
      default: field.default,
      type: field.type ?? null,
    })),
    inputs: block.input.map((input): InputDefJson => ({
      name: input.name,
      check: input.check,
      optional: input.optional,
      repeat: repeat(input.repeat),
      default: input.default.map(token),
      type: input.type ?? null,
      assign: input.assign ?? false,
    })),
    statements: block.statement.map((statement): StatementDefJson => ({
      name: statement.name,
      repeat: repeat(statement.repeat),
      when: statement.when,
    })),
    extra: block.extra.map((extra, index) => extraDef(extra, `${path}.extra[${String(index)}]`)),
    typeVars: { ...block.typeVars },
    order: (block.order ?? []).map(partName),
    placeVar: block.placeVar ?? null,
    members: { ...block.members },
    headers: [...block.headers],
    optionHeaders: Object.fromEntries(
      Object.entries(block.optionHeaders ?? {}).map(([field, options]) => [
        field,
        Object.fromEntries(
          Object.entries(options).map(([option, headers]) => [option, [...headers]] as const),
        ),
      ]),
    ),
  };
}

/**
 * The output pattern as the catalog writes it (types.md §4.2), checked in Rust; the editor reads
 * it (blockly-ext `types/pattern.ts`). Only an empty text is refused here.
 */
function output(value: string | null, path: string): OutputType | null {
  if (value === null) {
    return null;
  }
  if (value.trim() === '') {
    throw new CatalogJsonError(path, 'is an empty output type');
  }
  return value;
}

function partName(value: RustPartName): PartNameJson {
  return 'field' in value ? { field: value.field } : { input: value.input };
}

/**
 * A `types` entry as a type field's value: a name stays itself, any other term is its key, its
 * canonical compact JSON (`TypeTerm::key`, types.md §1.2). The reader built each object with one
 * key and arrays of two or more, so JSON.stringify writes the one canonical spelling.
 */
export function typeKey(term: RustTypeJson): string {
  return typeof term === 'string' ? term : JSON.stringify(term);
}

function repeat(value: RustRepeat | null): RepeatJson | null {
  return value === null ? null : { count: value.count, plus: value.plus };
}

/** Count defaults are text in the catalog ("1"); the editor gets numbers. */
function extraDef(extra: RustExtraDef, path: string): ExtraDefJson {
  const types = extra.types.map(typeKey);
  const typeClass = extra.typeClass ?? null;
  switch (extra.kind) {
    case 'count': {
      let countDefault: number | null = null;
      if (typeof extra.default === 'string' && /^(0|[1-9][0-9]{0,8})$/u.test(extra.default)) {
        countDefault = Number(extra.default);
      } else if (extra.default !== null) {
        throw new CatalogJsonError(`${path}.default`, 'should be a whole number written as text');
      }
      return {
        name: extra.name,
        kind: 'count',
        min: extra.min,
        max: extra.max,
        default: countDefault,
        types,
        typeClass,
      };
    }
    case 'flag':
      if (typeof extra.default === 'string') {
        throw new CatalogJsonError(`${path}.default`, 'should be true or false');
      }
      return {
        name: extra.name,
        kind: 'flag',
        min: null,
        max: null,
        default: extra.default,
        types,
        typeClass,
      };
    case 'params':
      return {
        name: extra.name,
        kind: 'params',
        min: null,
        max: extra.max,
        default: null,
        types,
        typeClass,
      };
  }
}

function token(value: RustToken): TokenJson {
  switch (value.kind) {
    case 'num':
      return { num: value.text };
    case 'str':
      return { str: value.text };
    case 'chr':
      return { chr: value.text };
    case 'ref':
      return { ref: value.text };
    case 'op':
      return { op: value.text };
    case 'kw':
      return { kw: value.text };
    case 'text':
      return { text: value.text };
  }
}

function category(value: RustToolboxCategory): ToolboxCategoryJson {
  return {
    id: value.id,
    name: value.name,
    icon: value.icon,
    colour: value.colour,
    dynamic: value.dynamic,
    entries: value.entry.map((entry): ToolboxEntryJson => ({
      block: entry.block,
      label: entry.label,
      preset: entry.preset === null ? null : preset(entry.preset),
    })),
  };
}

/** Only the keys the preset sets are present. */
function preset(value: RustPreset): PresetJson {
  const result: {
    fields?: Record<string, unknown>;
    extra?: Record<string, unknown>;
    inputs?: Record<string, readonly TokenJson[]>;
  } = {};
  if (value.fields.size > 0) {
    result.fields = Object.fromEntries(value.fields);
  }
  if (value.extra.size > 0) {
    result.extra = Object.fromEntries(value.extra);
  }
  if (value.inputs.size > 0) {
    result.inputs = Object.fromEntries(
      [...value.inputs].map(([name, tokens]) => [name, tokens.map(token)] as const),
    );
  }
  return result;
}
