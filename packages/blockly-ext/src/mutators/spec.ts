/**
 * What a mutator manages on a block, worked out from the block's catalog definition (03 §3.11.1).
 *
 * ## The label region
 *
 * A variadic block's friendly label has one *region* the mutator draws: from the first part that
 * names a repeated input or statement, or a statement that exists only while a flag is set, or the
 * repeat marker `…`, to the last such part, with everything between, plus the label words right
 * before it (`print`, `if`, `with`). Block registration draws the rest of the label and marks where
 * the region goes with an empty dummy input named `b2c_repeat` (`REPEAT_ANCHOR` in blockly-ext's
 * block registration); the mutator puts its inputs just before it. Without that marker, the
 * mutator puts them before the first input that holds a part after the region (or a statement
 * input the label does not mention), or at the end.
 *
 * | Block | Label | Region (drawn by the mutator) |
 * | --- | --- | --- |
 * | `io.print` | `print %ITEM … %SEP %NEWLINE` | `print %ITEM …` |
 * | `logic.operation` | `%ITEM %OP …` | `%ITEM %OP …` |
 * | `control.if` | `if %COND then %DO else if … else %ELSE` | all of it |
 * | `func.call` | `%FUNC %ARG …` | `%ARG …` |
 * | `func.define` | `define %NAME with … returns %RETURNS` | `with …` |
 *
 * Inside the region, the words before it are shown before the first copy of the group, the text
 * before a repeated part before every copy of it (`then` before each `DO`), the parts between the
 * last repeated part and `…` join one copy to the next (`else if`; `OP` between the items of
 * `logic.operation`), and the text before a flag-gated statement with it (`else`). A field that
 * joins copies (`OP`) is shown once, between the first two copies, and repeated as plain text
 * between later ones; when registration has already drawn it alone in an input, the mutator moves
 * that input there, and otherwise it creates the field itself.
 */
import type {
  BlockDefJson,
  ExtraDefJson,
  FieldDefJson,
  InputDefJson,
  LabelPart,
} from '../generated/catalog';
import { MutatorConfigError } from './errors';

/** Indices into `labelParts`, both inclusive. */
export interface LabelRegion {
  readonly start: number;
  readonly end: number;
}

/** A repeated input or statement: copies `NAME0` to `NAME{count + plus - 1}`. */
export interface MemberSpec {
  readonly name: string;
  readonly kind: 'value' | 'statement';
  /** Text shown before every copy. */
  readonly leading: readonly string[];
  /** The input definition, for value inputs. */
  readonly input: InputDefJson | null;
}

/** A statement input that exists while a flag is set (`ELSE` while `hasElse`). */
export interface GatedSpec {
  readonly name: string;
  readonly flag: string;
  /** Text shown with it. */
  readonly leading: readonly string[];
}

/** A count `extra`. */
export interface CountSpec {
  readonly name: string;
  readonly min: number;
  readonly max: number;
  readonly default: number;
}

/** A flag `extra`. */
export interface FlagSpec {
  readonly name: string;
  readonly default: boolean;
}

/** The parts a variadic mutator (items, if, call arguments) manages. */
export interface VariadicSpec {
  readonly kind: 'variadic';
  readonly blockType: string;
  /** The label words right before the region, shown before the first copy (`print`, `if`). */
  readonly groupLeading: readonly string[];
  /** The count that sets how many copies of the members there are. */
  readonly count: CountSpec;
  /** Copies = count + plus (`control.if` always has its first branch). */
  readonly plus: number;
  readonly members: readonly MemberSpec[];
  /** Text joining one copy to the next. */
  readonly joinTexts: readonly string[];
  /** A field joining one copy to the next (`logic.operation`'s `OP`). */
  readonly joinField: FieldDefJson | null;
  readonly flags: readonly FlagSpec[];
  readonly gated: readonly GatedSpec[];
  /** Fields and inputs after the region, in label order; the region goes before the first. */
  readonly anchorArgs: readonly string[];
}

/** The parts the parameter mutator manages. */
export interface ParamsSpec {
  readonly kind: 'params';
  readonly blockType: string;
  /** The label words right before the rows (`with`). */
  readonly groupLeading: readonly string[];
  /** The `params` extra key. */
  readonly name: string;
  /** The most rows. */
  readonly max: number;
  /** The parameter types, in menu order. */
  readonly types: readonly string[];
  readonly anchorArgs: readonly string[];
}

function isRepeat(part: LabelPart): part is { readonly repeat: true } {
  return 'repeat' in part;
}

function argOf(part: LabelPart): string | null {
  return 'arg' in part ? part.arg : null;
}

function textOf(part: LabelPart): string | null {
  return 'text' in part ? part.text : null;
}

/** The names of the repeated inputs and statements and of the flag-gated statements. */
function variableArgs(def: BlockDefJson): Set<string> {
  const names = new Set<string>();
  for (const input of def.inputs) {
    if (input.repeat !== null) {
      names.add(input.name);
    }
  }
  for (const statement of def.statements) {
    if (statement.repeat !== null || statement.when !== null) {
      names.add(statement.name);
    }
  }
  return names;
}

/** The region without the label words before it: from the first variable part to the last. */
function coreRegion(def: BlockDefJson): LabelRegion | null {
  const variable = variableArgs(def);
  let start = -1;
  let end = -1;
  for (const [index, part] of def.labelParts.entries()) {
    const arg = argOf(part);
    if (isRepeat(part) || (arg !== null && variable.has(arg))) {
      if (start === -1) {
        start = index;
      }
      end = index;
    }
  }
  return start === -1 ? null : { start, end };
}

/** The label words right before the core region, in order. */
function wordsBefore(def: BlockDefJson, core: LabelRegion): string[] {
  const words: string[] = [];
  for (let index = core.start - 1; index >= 0; index -= 1) {
    const part = def.labelParts[index];
    const text = part === undefined ? null : textOf(part);
    if (text === null) {
      break;
    }
    words.unshift(text);
  }
  return words;
}

/**
 * The part of a block's friendly label that its mutator draws (see the module comment), as indices
 * into `labelParts`, or `null` for a block without repeated or flag-gated parts. Block
 * registration draws the label parts outside this region.
 */
export function mutatorLabelRegion(def: BlockDefJson): LabelRegion | null {
  const core = coreRegion(def);
  return core === null
    ? null
    : { start: core.start - wordsBefore(def, core).length, end: core.end };
}

function fail(def: BlockDefJson, message: string): never {
  throw new MutatorConfigError(def.id, `${def.id}: ${message}`);
}

/** The fields and inputs after the region, then the statements the label does not mention. */
function anchorArgs(def: BlockDefJson, region: LabelRegion): string[] {
  const after = def.labelParts
    .slice(region.end + 1)
    .map(argOf)
    .filter((arg): arg is string => arg !== null);
  return [...after, ...def.statementsNotInLabel];
}

function countSpec(def: BlockDefJson, extra: ExtraDefJson | undefined): CountSpec {
  if (extra?.kind !== 'count') {
    return fail(def, 'a repeated part must be counted by a count extra');
  }
  const { min, max } = extra;
  const value = extra.default;
  if (min === null || max === null || typeof value !== 'number' || min > max) {
    return fail(def, `the count ${extra.name} needs a minimum, a maximum and a default`);
  }
  return { name: extra.name, min, max, default: value };
}

/** The spec of a variadic block (items, *if* or call arguments). */
export function variadicSpec(def: BlockDefJson): VariadicSpec {
  const region = coreRegion(def);
  const marker = def.labelParts.findIndex(isRepeat);
  if (region === null || marker === -1) {
    return fail(def, 'the label has no repeated group (…)');
  }
  const inputs = new Map(def.inputs.map((input) => [input.name, input]));
  const statements = new Map(def.statements.map((statement) => [statement.name, statement]));
  const repeatOf = (name: string): { count: string; plus: number } | null =>
    inputs.get(name)?.repeat ?? statements.get(name)?.repeat ?? null;

  const regionParts = def.labelParts.slice(region.start, region.end + 1);
  const markerAt = marker - region.start;
  const lastMemberAt = regionParts.reduce((last, part, index) => {
    const arg = argOf(part);
    return index < markerAt && arg !== null && repeatOf(arg) !== null ? index : last;
  }, -1);

  const members: MemberSpec[] = [];
  const joinTexts: string[] = [];
  let joinField: FieldDefJson | null = null;
  const gated: GatedSpec[] = [];
  let pending: string[] = [];
  let repeat: { count: string; plus: number } | null = null;

  for (const [index, part] of regionParts.entries()) {
    const text = textOf(part);
    const arg = argOf(part);
    if (index === markerAt) {
      // Text just before `…` joins one copy of the group to the next (`else if`).
      joinTexts.push(...pending);
      pending = [];
    } else if (text !== null) {
      pending.push(text);
    } else if (arg !== null && index <= lastMemberAt) {
      const memberRepeat = repeatOf(arg);
      if (memberRepeat === null) {
        fail(def, `${arg} sits between repeated parts but is not repeated`);
      }
      if (
        repeat !== null &&
        (repeat.count !== memberRepeat.count || repeat.plus !== memberRepeat.plus)
      ) {
        fail(def, 'all repeated parts must use the same count');
      }
      repeat = memberRepeat;
      const input = inputs.get(arg) ?? null;
      members.push({
        name: arg,
        kind: input === null ? 'statement' : 'value',
        leading: pending,
        input,
      });
      pending = [];
    } else if (arg !== null && index < markerAt) {
      const field = def.fields.find((candidate) => candidate.name === arg);
      if (field === undefined || joinField !== null) {
        fail(def, `only one field may join repeated parts (${arg})`);
      }
      joinTexts.push(...pending);
      pending = [];
      joinField = field;
    } else if (arg !== null) {
      const when = statements.get(arg)?.when ?? null;
      if (when === null) {
        fail(def, `${arg} follows the repeated group but is not a flag-gated statement`);
      }
      gated.push({ name: arg, flag: when, leading: pending });
      pending = [];
    }
  }

  if (repeat === null) {
    return fail(def, 'the repeated group names no repeated input or statement');
  }
  const countName = repeat.count;
  const count = countSpec(
    def,
    def.extra.find((extra) => extra.name === countName),
  );
  const flags: FlagSpec[] = def.extra
    .filter((extra) => extra.kind === 'flag')
    .map((extra) => ({ name: extra.name, default: extra.default === true }));
  for (const part of gated) {
    if (!flags.some((flag) => flag.name === part.flag)) {
      fail(def, `${part.name} depends on ${part.flag}, which is not a flag extra`);
    }
  }
  if (def.extra.some((extra) => extra.kind === 'params')) {
    fail(def, 'a variadic mutator cannot also manage parameters');
  }
  return {
    kind: 'variadic',
    blockType: def.id,
    groupLeading: wordsBefore(def, region),
    count,
    plus: repeat.plus,
    members,
    joinTexts,
    joinField,
    flags,
    gated,
    anchorArgs: anchorArgs(def, region),
  };
}

/** The spec of a block with parameter rows (`func.define`). */
export function paramsSpec(def: BlockDefJson): ParamsSpec {
  const params = def.extra.filter((extra) => extra.kind === 'params');
  const [extra] = params;
  const region = coreRegion(def);
  if (params.length !== 1 || extra === undefined) {
    return fail(def, 'the parameter mutator needs exactly one params extra');
  }
  if (extra.max === null || extra.types.length === 0) {
    return fail(def, 'the params extra needs types and a maximum');
  }
  if (def.extra.length !== 1 || region === null || region.start !== region.end) {
    return fail(def, 'the parameter mutator needs a label with … and no other variable part');
  }
  return {
    kind: 'params',
    blockType: def.id,
    groupLeading: wordsBefore(def, region),
    name: extra.name,
    max: extra.max,
    types: extra.types,
    anchorArgs: anchorArgs(def, region),
  };
}
