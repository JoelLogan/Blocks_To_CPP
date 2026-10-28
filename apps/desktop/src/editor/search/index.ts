/**
 * The block search of Quick Insert and toolbox search (docs/spec/03-block-language.md §3.14, 04
 * §4.2; docs/plans/m3-contracts/w4-editor-tools.md §2.3): one entry per block the toolbox offers
 * in the editor's mode, with the module's variables (their getters) and functions (their calls),
 * each with the texts it is found by. An entry's item is the toolbox's own (presets, start values,
 * the label mode), so a block inserted or dragged from the results is the toolbox's block.
 */
import { catalogBlock, type BlockDefJson } from '@blocks2cpp/blockly-ext';

import {
  categoryContents,
  displayName,
  shownToolboxCategories,
  type ContentsContext,
} from '../toolbox/contents';
import { B2C_BLOCK_KIND, type B2cBlockInfo } from '../toolbox/presets';
import { entryScore, queryWords, type WeightedText } from './fuzzy';
import { SYNONYMS } from './synonyms';

export { entryScore, queryWords, wordScore, type WeightedText } from './fuzzy';
export { SYNONYMS } from './synonyms';

/** One block the search finds. */
export interface SearchEntry {
  /** The toolbox item of the block. */
  readonly item: B2cBlockInfo;
  /** What a result reads: the block's label with `…` for its parts, or the symbol's name. */
  readonly title: string;
  /** The name of the category it is in. */
  readonly category: string;
  /** The texts it is found by, in lower case, with their weights. */
  readonly texts: readonly WeightedText[];
}

/** The most results Quick Insert lists. */
export const MAX_RESULTS = 12;

/** A label template with each `%NAME` (and a repeat `…`) read as `…`, spaces made single. */
export function templateTitle(template: string): string {
  return template
    .replace(/%[A-Z][A-Z0-9_]*/gu, '…')
    .replace(/\s+/gu, ' ')
    .trim();
}

/** The first sentence of a help text. */
function firstSentence(help: string): string {
  const end = help.search(/[.!?](\s|$)/u);
  return end < 0 ? help : help.slice(0, end + 1);
}

/** The symbol a reference field of an item names, by its preset `{ref}`. */
function presetRef(item: B2cBlockInfo, field: string): string | null {
  const value = (item.fields as Record<string, unknown> | undefined)?.[field];
  return typeof value === 'object' && value !== null && 'ref' in value ? String(value.ref) : null;
}

/** The entry of one toolbox item. */
function entryOf(
  item: B2cBlockInfo,
  def: BlockDefJson,
  category: string,
  names: ReadonlyMap<string, string>,
): SearchEntry {
  const symbolField = def.id === 'var.get' ? 'VAR' : def.id.startsWith('func.call') ? 'FUNC' : null;
  const ref = symbolField === null ? null : presetRef(item, symbolField);
  const name = ref === null ? null : (names.get(ref) ?? null);
  const label = templateTitle(def.label.friendly);
  const title =
    name === null
      ? label
      : def.id === 'var.get'
        ? displayName(name)
        : `${displayName(name)} (call)`;
  const texts: WeightedText[] = [
    { text: title.toLowerCase(), weight: 1 },
    { text: label.toLowerCase(), weight: 0.9 },
    { text: templateTitle(def.label.cpp).toLowerCase(), weight: 0.9 },
    { text: (SYNONYMS[def.id]?.[0] ?? '').toLowerCase(), weight: 1.05 },
    { text: (SYNONYMS[def.id] ?? []).join(' ').toLowerCase(), weight: 0.8 },
    { text: firstSentence(def.help).toLowerCase(), weight: 0.4 },
    { text: category.toLowerCase(), weight: 0.4 },
  ];
  return { item, title, category, texts };
}

/**
 * Every block the toolbox offers now, as search entries, in toolbox order: the catalog's blocks of
 * each category shown in the mode, the module's variables and its functions.
 */
export function searchEntries(context: ContentsContext): SearchEntry[] {
  const names = new Map(
    context.symbols.allSymbols().map((symbol) => [symbol.id, symbol.name] as const),
  );
  const entries: SearchEntry[] = [];
  for (const category of shownToolboxCategories(context)) {
    for (const item of categoryContents(category, context)) {
      if ((item as { kind?: unknown }).kind !== B2C_BLOCK_KIND) {
        continue;
      }
      const block = item as B2cBlockInfo;
      const def = catalogBlock(block.type);
      if (def !== undefined) {
        entries.push(entryOf(block, def, category.name, names));
      }
    }
  }
  return entries;
}

/** The entries that match `query`, best first (ties: the shorter title, then toolbox order), at most `limit`. */
export function searchBlocks(
  entries: readonly SearchEntry[],
  query: string,
  limit = MAX_RESULTS,
): SearchEntry[] {
  const words = queryWords(query);
  if (words.length === 0) {
    return [];
  }
  return entries
    .map((entry, index) => ({ entry, index, score: entryScore(words, entry.texts) }))
    .filter(({ score }) => score > 0)
    .sort(
      (a, b) =>
        b.score - a.score || a.entry.title.length - b.entry.title.length || a.index - b.index,
    )
    .slice(0, limit)
    .map(({ entry }) => entry);
}
