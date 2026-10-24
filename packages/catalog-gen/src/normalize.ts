// Turns the Rust (serde) shapes of catalog.json into the editor's shapes (catalog-types.ts):
// plural, camelCase names (`field` becomes `fields`), numbers for count defaults (`"1"` becomes
// 1), null for keys that do not apply to a kind, and parsed friendly labels.

import type {
  BlockDefJson,
  ExtraDefJson,
  FieldDefJson,
  InputDefJson,
  OutputType,
  PresetJson,
  RepeatJson,
  StatementDefJson,
  TokenJson,
  ToolboxCategoryJson,
  ToolboxEntryJson,
} from './catalog-types.ts';
import {
  CatalogJsonError,
  type CatalogJson,
  type RustBlockDef,
  type RustExtraDef,
  type RustPreset,
  type RustRepeat,
  type RustToken,
  type RustToolboxCategory,
} from './catalog-json.ts';
import { labelArgs, parseLabel } from './labels.ts';

/** Everything the generated module exports. */
export interface Model {
  readonly catalogVersion: string;
  readonly blocks: readonly BlockDefJson[];
  readonly toolbox: readonly ToolboxCategoryJson[];
}

/** Converts the export into the editor's shapes. Block order (by ID) is kept. */
export function normalize(input: CatalogJson): Model {
  return {
    catalogVersion: input.catalogVersion,
    blocks: input.blocks.map((block, index) => blockDef(block, `blocks[${String(index)}]`)),
    toolbox: input.toolbox.category.map(category),
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
      types: [...field.types],
      default: field.default,
    })),
    inputs: block.input.map((input): InputDefJson => ({
      name: input.name,
      check: input.check,
      optional: input.optional,
      repeat: repeat(input.repeat),
      default: input.default.map(token),
    })),
    statements: block.statement.map((statement): StatementDefJson => ({
      name: statement.name,
      repeat: repeat(statement.repeat),
      when: statement.when,
    })),
    extra: block.extra.map((extra, index) => extraDef(extra, `${path}.extra[${String(index)}]`)),
  };
}

function output(value: string | null, path: string): OutputType | null {
  if (value === null) {
    return null;
  }
  switch (value) {
    case 'any':
    case 'bool':
    case 'int':
    case 'double':
    case 'number':
    case 'char':
    case 'string':
    case 'symbol':
      return value;
    default:
      if (value.startsWith('field:')) {
        return `field:${value.slice('field:'.length)}`;
      }
      throw new CatalogJsonError(path, `is not an output type (${JSON.stringify(value)})`);
  }
}

function repeat(value: RustRepeat | null): RepeatJson | null {
  return value === null ? null : { count: value.count, plus: value.plus };
}

/** Count defaults are text in the catalog ("1"); the editor gets numbers. */
function extraDef(extra: RustExtraDef, path: string): ExtraDefJson {
  const types = [...extra.types];
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
      };
    case 'params':
      return { name: extra.name, kind: 'params', min: null, max: extra.max, default: null, types };
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
