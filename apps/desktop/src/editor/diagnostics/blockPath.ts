/**
 * Block paths for the Problems panel (docs/spec/04-user-interface.md §4.4, M2 decision "Block paths
 * in Problems"): each level from the top-level block down to the diagnostic's block, named by a
 * short label: `main` for the program block, the function's name for a definition, and for other
 * blocks the friendly label up to its first placeholder with the current dropdown text filled in
 * (`main › repeat until › if`).
 *
 * A label that would be empty because the block's friendly label starts with a placeholder
 * (`%A %OP %B`, `%VAR`) is the whole label instead, with names, values and dropdown text filled in
 * and `…` for inputs (`… < …`, `guess`). Before the editor has provided the block catalog
 * (./catalog.ts), and for a type the catalog does not have, a block is named by its type.
 *
 * The document is untrusted data: everything is walked iteratively with explicit bounds, and all
 * project text is shown with hidden characters made visible and long text shortened. This module
 * does not load Blockly (see ./catalog.ts).
 */
import type { BdmBlock, BdmDocument, BdmModule, FieldValue } from '@blocks2cpp/b2c-core-wasm';
import type { BlockDefJson } from '@blocks2cpp/blockly-ext';

import { placeholderFor, visibleInvisibles } from '../../panels/shared/invisibles';
import { type PathCatalog, pathCatalog } from './catalog';

/** What separates the levels of a block path. */
export const PATH_SEPARATOR = ' › ';

/** The longest label of one level, in code points. */
export const MAX_LABEL_CHARS = 40;

/** The most levels a path shows; deeper paths keep the first two and the last five. */
export const MAX_PATH_LEVELS = 8;

/** How many levels a shortened path keeps at its start and at its end. */
const KEPT_AT_START = 2;
const KEPT_AT_END = 5;

/** The longest substituted value inside a label (a text literal, a name), in code points. */
const MAX_VALUE_CHARS = 24;

/** Where a block sits in the document. */
export interface BlockPlace {
  readonly block: BdmBlock;
  /** The block it is nested in (through an input or a statement list), or null at the top. */
  readonly parent: string | null;
  readonly module: BdmModule;
}

/** A document indexed for paths: every block by ID, and the names of the symbols it declares. */
export interface DocumentIndex {
  readonly blocks: ReadonlyMap<string, BlockPlace>;
  readonly names: ReadonlyMap<string, string>;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isBlock(value: unknown): value is BdmBlock {
  return isObject(value) && typeof value['id'] === 'string' && typeof value['type'] === 'string';
}

/** The symbol a declaration field value (`{sym, name}`) declares, or null for other values. */
function declaration(value: unknown): { readonly sym: string; readonly name: string } | null {
  if (!isObject(value)) {
    return null;
  }
  const { sym, name } = value;
  return typeof sym === 'string' && typeof name === 'string' ? { sym, name } : null;
}

/** The symbol a reference field value (`{ref}`) names, or null for other values. */
function reference(value: unknown): string | null {
  return isObject(value) && typeof value['ref'] === 'string' ? value['ref'] : null;
}

/** Records the symbols a block declares: its declaration fields and its parameter rows. */
function collectNames(block: BdmBlock, names: Map<string, string>): void {
  for (const value of Object.values(block.fields ?? {})) {
    const declared = declaration(value);
    if (declared !== null) {
      names.set(declared.sym, declared.name);
    }
  }
  const params = block.extra?.['params'];
  if (Array.isArray(params)) {
    for (const row of params) {
      const declared = declaration(row);
      if (declared !== null) {
        names.set(declared.sym, declared.name);
      }
    }
  }
}

/**
 * Indexes every block of every module (nested blocks, statement lists and loose stacks), without
 * recursion. A loose stack's blocks are top-level: they hang below their head on the canvas, not
 * inside it. The first block with an ID wins (a loaded document never repeats one).
 */
export function indexDocument(doc: BdmDocument): DocumentIndex {
  const blocks = new Map<string, BlockPlace>();
  const names = new Map<string, string>();
  const pending: { block: unknown; parent: string | null; module: BdmModule }[] = [];
  const modules: unknown = doc.modules;
  for (const module of Array.isArray(modules) ? (modules as BdmModule[]) : []) {
    const workspace: unknown = isObject(module) ? module.workspace : undefined;
    const top: unknown = isObject(workspace) ? workspace['blocks'] : undefined;
    for (const block of Array.isArray(top) ? (top as unknown[]) : []) {
      pending.push({ block, parent: null, module });
    }
  }
  for (let next = pending.pop(); next !== undefined; next = pending.pop()) {
    const { block, parent, module } = next;
    if (!isBlock(block) || blocks.has(block.id)) {
      continue;
    }
    blocks.set(block.id, { block, parent, module });
    collectNames(block, names);
    for (const input of Object.values(block.inputs ?? {})) {
      if (isObject(input) && 'block' in input) {
        pending.push({ block: input.block, parent: block.id, module });
      }
    }
    for (const list of Object.values(block.statements ?? {})) {
      for (const child of Array.isArray(list) ? (list as unknown[]) : []) {
        pending.push({ block: child, parent: block.id, module });
      }
    }
    const stack: unknown = block.stack;
    for (const stacked of Array.isArray(stack) ? (stack as unknown[]) : []) {
      pending.push({ block: stacked, parent: null, module });
    }
  }
  return { blocks, names };
}

/**
 * Shortens `text` to at most `max` code points, ending with `…` when it was cut. The cut never
 * splits a surrogate pair.
 */
export function shorten(text: string, max: number): string {
  if (max < 1 || text.length <= max) {
    return text;
  }
  let units = 0;
  for (let points = 0; points < max - 1 && units < text.length; points++) {
    const unit = text.charCodeAt(units);
    units += unit >= 0xd800 && unit <= 0xdbff && units + 1 < text.length ? 2 : 1;
  }
  const rest = text.length - units;
  const lead = text.charCodeAt(units);
  const restIsOneCodePoint = rest === 1 || (rest === 2 && lead >= 0xd800 && lead <= 0xdbff);
  return rest === 0 || restIsOneCodePoint ? text : `${text.slice(0, units)}…`;
}

/** Project text inside a label: hidden characters visible, on one line, shortened. */
function shown(text: string, max = MAX_VALUE_CHARS): string {
  const visible = visibleInvisibles(text).replace(/[\t\n]/g, (character) =>
    placeholderFor(character.charCodeAt(0)),
  );
  return shorten(visible, max);
}

/** A part of a friendly label template: text, an argument (`%NAME`) or a repeated group (`…`). */
type TemplatePart =
  { readonly text: string } | { readonly arg: string } | { readonly repeat: true };

/**
 * Splits a friendly label template (03 §3.11.1: `%NAME` arguments, a bare `…` for a group),
 * keeping the spaces between the parts (the catalog's parsed `labelParts` trims them).
 */
function templateParts(template: string): TemplatePart[] {
  const parts: TemplatePart[] = [];
  let last = 0;
  for (const match of template.matchAll(/%([A-Z][A-Z0-9_]*)|…/g)) {
    if (match.index > last) {
      parts.push({ text: template.slice(last, match.index) });
    }
    const name = match[1];
    parts.push(name === undefined ? { repeat: true } : { arg: name });
    last = match.index + match[0].length;
  }
  if (last < template.length) {
    parts.push({ text: template.slice(last) });
  }
  return parts;
}

/** The text a dropdown or type field shows for its value, or null for other kinds of argument. */
function choiceText(
  catalog: PathCatalog,
  def: BlockDefJson,
  name: string,
  value: FieldValue | undefined,
): string | null {
  const field = def.fields.find((candidate) => candidate.name === name);
  if (field === undefined) {
    return null;
  }
  const current = value ?? field.default;
  if (typeof current !== 'string') {
    return null;
  }
  if (field.kind === 'dropdown') {
    const option = field.options.find(([, optionValue]) => optionValue === current);
    return option === undefined ? shown(current) : option[0];
  }
  if (field.kind === 'type') {
    return shown(catalog.typeName(current));
  }
  return null;
}

/** The text a field shows in the whole-label fallback: names, values and choices. */
function fieldText(
  catalog: PathCatalog,
  def: BlockDefJson,
  block: BdmBlock,
  name: string,
  names: ReadonlyMap<string, string>,
): string {
  const value = block.fields?.[name];
  const choice = choiceText(catalog, def, name, value);
  if (choice !== null) {
    return choice;
  }
  const field = def.fields.find((candidate) => candidate.name === name);
  if (field === undefined) {
    // Not a field: an input or a statement list.
    return '…';
  }
  const declared = declaration(value);
  if (declared !== null) {
    return shown(declared.name);
  }
  const ref = reference(value);
  if (ref !== null) {
    const known = names.get(ref);
    return known === undefined ? 'missing' : shown(known);
  }
  if (typeof value === 'string') {
    return shown(value);
  }
  if (field.kind === 'checkbox') {
    return '';
  }
  return typeof field.default === 'string' ? shown(field.default) : '…';
}

/** Collapses runs of spaces and of `…` placeholders, and trims. */
function tidy(text: string): string {
  return text
    .replace(/\s+/g, ' ')
    .replace(/…(?: …)+/g, '…')
    .trim();
}

/** The label up to its first placeholder that is not a dropdown or a type. */
function labelPrefix(catalog: PathCatalog, def: BlockDefJson, block: BdmBlock): string {
  let text = '';
  for (const part of templateParts(def.label.friendly)) {
    if ('text' in part) {
      text += part.text;
      continue;
    }
    if ('repeat' in part) {
      break;
    }
    const choice = choiceText(catalog, def, part.arg, block.fields?.[part.arg]);
    if (choice === null) {
      break;
    }
    text += choice;
  }
  // Words that lead into the cut-off placeholder ('letter '' and the like) go too.
  return tidy(text).replace(/[\s'"([{=,:]+$/u, '');
}

/** The whole label with every placeholder filled in (the fallback for labels that start with one). */
function labelFilled(
  catalog: PathCatalog,
  def: BlockDefJson,
  block: BdmBlock,
  names: ReadonlyMap<string, string>,
): string {
  let text = '';
  for (const part of templateParts(def.label.friendly)) {
    if ('text' in part) {
      text += part.text;
    } else if ('repeat' in part) {
      text += '…';
    } else {
      text += fieldText(catalog, def, block, part.arg, names);
    }
  }
  return tidy(text);
}

/**
 * The short label of one block for a block path: `main`, a function's name, or the friendly label
 * up to its first placeholder with dropdown text filled in (`repeat until`). Without a catalog
 * (`catalog` defaults to the one the editor provided), or for a type the catalog does not have, it
 * is the block's type.
 */
export function blockLabel(
  block: BdmBlock,
  names: ReadonlyMap<string, string>,
  catalog: PathCatalog | null = pathCatalog(),
): string {
  if (block.type === 'program.main') {
    return 'main';
  }
  if (block.type === 'func.define') {
    const declared = declaration(block.fields?.['NAME']);
    return declared === null ? 'function' : shown(declared.name, MAX_LABEL_CHARS);
  }
  const def = catalog?.block(block.type);
  if (catalog === null || def === undefined) {
    return shown(block.type, MAX_LABEL_CHARS);
  }
  const prefix = labelPrefix(catalog, def, block);
  const label = prefix === '' ? labelFilled(catalog, def, block, names) : prefix;
  return shorten(label === '' ? def.id : label, MAX_LABEL_CHARS);
}

/**
 * The path of a block, from its top-level block down to it (`main › repeat until › if`), or an
 * empty string when the document has no such block. Long paths keep their first and last levels
 * around an `…`. `catalog` defaults to the one the editor provided.
 */
export function blockPath(
  index: DocumentIndex,
  blockId: string,
  catalog: PathCatalog | null = pathCatalog(),
): string {
  const labels: string[] = [];
  const seen = new Set<string>();
  for (
    let place = index.blocks.get(blockId);
    place !== undefined && !seen.has(place.block.id);
    place = place.parent === null ? undefined : index.blocks.get(place.parent)
  ) {
    seen.add(place.block.id);
    labels.push(blockLabel(place.block, index.names, catalog));
  }
  labels.reverse();
  if (labels.length > MAX_PATH_LEVELS) {
    labels.splice(KEPT_AT_START, labels.length - KEPT_AT_START - KEPT_AT_END, '…');
  }
  return labels.join(PATH_SEPARATOR);
}

/** The module a block is in, or null. */
export function moduleOf(index: DocumentIndex, blockId: string): BdmModule | null {
  return index.blocks.get(blockId)?.module ?? null;
}
