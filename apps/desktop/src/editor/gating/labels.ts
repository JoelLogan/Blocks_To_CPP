/**
 * Block labels in the two display modes (docs/spec/03-block-language.md §3.5.2): which template a
 * block shows, the template split into parts as the catalog generator splits the Friendly one
 * (`labelParts` in the generated catalog), and the C++ text of the operator and function menus.
 * Both modes give the same code; only the text differs.
 *
 * The block code applies these after W2's integration (the W4 contract,
 * docs/plans/m3-contracts/w4-appearance-expert.md §6.1).
 */
import type { BlockDefJson, LabelPart } from '@blocks2cpp/blockly-ext';

import type { LabelMode } from './gate';

/** `%NAME`, with the catalog's name rule: `[A-Z][A-Z0-9_]*`. */
const ARG = /^%([A-Z][A-Z0-9_]*)/;

/** The repeat marker, as a word of its own. */
const REPEAT = '…';

/** The label template a block shows in `mode`. */
export function labelTemplate(def: Pick<BlockDefJson, 'label'>, mode: LabelMode): string {
  return mode === 'cpp' ? def.label.cpp : def.label.friendly;
}

/** How {@link labelParts} reads a template. */
export interface LabelPartsOptions {
  /**
   * A C++ template: a `…` that is the whole of a `{ … }` stands for a statement body, not for a
   * repeated group, so it stays text (`while (true) { … }`). Statement inputs follow the label
   * in both modes, and a repeat part there would take the label's words into a repeated group.
   */
  readonly cpp?: boolean;
}

/**
 * Splits a label template into text, argument and repeat parts, exactly as the catalog generator
 * splits the Friendly label (packages/catalog-gen/src/labels.ts): text parts are trimmed, runs of
 * whitespace become one space, whitespace-only text is dropped, a `%` that does not start a name is
 * text, and `…` is a repeat part only as a word of its own (and, in a C++ template, not as a
 * statement body, {@link LabelPartsOptions.cpp}). Punctuation stays with its text, so
 * `%LIST.push_back(%VALUE);` gives `LIST`, `.push_back(`, `VALUE` and `);`.
 */
export function labelParts(template: string, options: LabelPartsOptions = {}): LabelPart[] {
  const parts: LabelPart[] = [];
  let text = '';
  const flush = (): void => {
    const trimmed = text.trim().replace(/\s+/gu, ' ');
    if (trimmed !== '') {
      parts.push({ text: trimmed });
    }
    text = '';
  };
  let index = 0;
  while (index < template.length) {
    const arg = ARG.exec(template.slice(index));
    if (arg?.[1] !== undefined) {
      flush();
      parts.push({ arg: arg[1] });
      index += arg[0].length;
      continue;
    }
    const char = template.charAt(index);
    if (
      char === REPEAT &&
      isWordEdge(template, index - 1) &&
      isWordEdge(template, index + 1) &&
      !(options.cpp === true && isStatementBody(template, index))
    ) {
      flush();
      parts.push({ repeat: true });
    } else {
      text += char;
    }
    index += 1;
  }
  flush();
  return parts;
}

/** Whether the `…` at `index` is the whole of a `{ … }`. */
function isStatementBody(template: string, index: number): boolean {
  return (
    template.slice(0, index).trimEnd().endsWith('{') &&
    template
      .slice(index + 1)
      .trimStart()
      .startsWith('}')
  );
}

/** Whether the position is outside the template or holds whitespace. */
function isWordEdge(template: string, index: number): boolean {
  return index < 0 || index >= template.length || /\s/u.test(template.charAt(index));
}

/** What {@link blockLabelParts} reads from a block definition. */
type LabelledDef = Pick<BlockDefJson, 'label' | 'labelParts' | 'fields' | 'inputs'>;

/**
 * The parts of the label a block shows in `mode`. In the C++ mode a block whose C++ template names
 * every field and value input is laid out from it; any other keeps the Friendly layout (with the
 * options' C++ texts, {@link optionLabel}), since its template stands for one form of the block
 * only (`std::round(%VALUE)` for every function of `%FN of %VALUE`). Statement inputs need not be
 * named: they follow the label in both modes.
 */
export function blockLabelParts(def: LabelledDef, mode: LabelMode): readonly LabelPart[] {
  return mode === 'cpp' && hasFullCppLabel(def)
    ? labelParts(def.label.cpp, { cpp: true })
    : def.labelParts;
}

/** Whether the C++ template of `def` names every field and value input. */
export function hasFullCppLabel(def: Pick<BlockDefJson, 'label' | 'fields' | 'inputs'>): boolean {
  return partsMissingFromCppLabel({ ...def, statements: [] }).length === 0;
}

/**
 * The fields, value inputs and statement inputs of `def` that its C++ template does not name (the
 * W4 contract's §6.1 lists the blocks, for the catalog's C++ labels).
 */
export function partsMissingFromCppLabel(
  def: Pick<BlockDefJson, 'label' | 'fields' | 'inputs' | 'statements'>,
): string[] {
  const named = new Set(
    labelParts(def.label.cpp).flatMap((part) => ('arg' in part ? [part.arg] : [])),
  );
  return [...def.fields, ...def.inputs, ...def.statements]
    .map((part) => part.name)
    .filter((name) => !named.has(name));
}

/**
 * The C++ text of dropdown options, by block, field and option value: the operator menus (03 §3.7,
 * "in C++ label mode every option shows its operator") and the math functions' menus (every `FN`
 * menu of the catalog; w4-appearance-expert.md §6.1). An option not listed keeps its Friendly text.
 */
export const CPP_OPTION_LABELS: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  'math.arithmetic.OP': { add: '+', sub: '-', mul: '*', div: '/', mod: '%' },
  'math.compare.OP': { lt: '<', le: '<=', gt: '>', ge: '>=', eq: '==', ne: '!=' },
  'logic.operation.OP': { and: '&&', or: '||' },
  'var.change.OP': {
    add: '+=',
    sub: '-=',
    mul: '*=',
    div: '/=',
    mod: '%=',
    append: '+=',
    and: '&=',
    or: '|=',
    xor: '^=',
    shl: '<<=',
    shr: '>>=',
  },
  'math.fn.FN': {
    floor: 'std::floor',
    ceil: 'std::ceil',
    round: 'std::round',
    sqrt: 'std::sqrt',
    cbrt: 'std::cbrt',
    exp: 'std::exp',
    log: 'std::log',
    log10: 'std::log10',
    sin: 'std::sin',
    cos: 'std::cos',
    tan: 'std::tan',
    asin: 'std::asin',
    acos: 'std::acos',
    atan: 'std::atan',
  },
  'math.minmax.FN': { max: 'std::max', min: 'std::min' },
  'math.fn_two.FN': { pow: 'std::pow', atan2: 'std::atan2', hypot: 'std::hypot' },
};

/** The text an option shows in `mode`: its C++ text in the C++ mode when it has one. */
export function optionLabel(
  blockType: string,
  field: string,
  value: string,
  friendly: string,
  mode: LabelMode,
): string {
  if (mode !== 'cpp') {
    return friendly;
  }
  const table = CPP_OPTION_LABELS[`${blockType}.${field}`];
  return (
    (table !== undefined && Object.hasOwn(table, value) ? table[value] : undefined) ?? friendly
  );
}
