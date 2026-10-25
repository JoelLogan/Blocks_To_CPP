/**
 * The editor's copy of typed slots (ADR-0018, docs/plans/m3-contracts/slots.md §2, §4.2), mirroring
 * `b2c_ir::sig::{PartType, BlockParts}` and `b2c_lang::{type_block, SlotFit, Binder, BlockTyping}`.
 *
 * {@link typeBlock} types one block from its catalog signature, its parts' types, the type its place
 * expects and the facts about user types, by the same one-pass rule as the analyser. The slot
 * fixture that `b2c-lang` writes (`./slot-fixture.json`) holds the two copies together;
 * `./slots.test.ts` runs every case of it.
 */
import type { TokenJson } from '../generated/catalog';
import {
  blockSignature,
  type BlockSigTs,
  type Fit,
  type InputSigTs,
  type PartName,
  type SignatureDef,
  TypeEnv,
  type TypePattern,
} from './pattern';
import {
  checkAccepts,
  ERROR_TYPE,
  sameType,
  staticAssignment,
  staticConversion,
  type StaticConversion,
  type StaticType,
  type TypeFactsTs,
} from './static-type';

/** What one part of a block is when the block is typed. */
export type PartType =
  /** Nothing chosen or connected, or a disabled block. */
  | { readonly kind: 'empty' }
  /** `auto` in a type field. */
  | { readonly kind: 'auto' }
  /** A value that takes its type from its place: typed after the block. */
  | { readonly kind: 'fromPlace' }
  /** A known type; `error` binds the variable to `error` and fits everything. */
  | { readonly kind: 'known'; readonly type: StaticType }
  /** Not known to the editor (something the analysis has not seen). */
  | { readonly kind: 'unknown' };

/** The parts of the block being typed. A repeated input is asked part by part (`ITEM2`). */
export interface BlockParts {
  field(name: string): PartType;
  input(part: string): PartType;
  /** How many parts the repeated input `base` has now. */
  count(base: string): number;
  /** `param:F`: parameter `index` of the function chosen in field `field`. */
  param(field: string, index: number): PartType;
  /** `member:F`: field `index` of the struct chosen in field `field`. */
  member(field: string, index: number): PartType;
  /** A `members` field: the chosen member's type when it is a member of `owner`. */
  chosenMember(field: string, owner: StaticType): PartType;
  /** `result`: what a `return` gives back where the block is. */
  result(): PartType;
}

/** `b2c_lang::SlotFit`: a conversion that fits, `invalid`, `outsideClass` or `unknown`. */
export type SlotFit = StaticConversion | 'outsideClass' | 'unknown';

/** Which part bound a type variable, for messages. */
export type Binder =
  | { readonly kind: 'field'; readonly name: string }
  | { readonly kind: 'input'; readonly name: string }
  | { readonly kind: 'place' };

/** The result of typing one block (`b2c_lang::BlockTyping`). */
export interface BlockTyping {
  /** Each bound variable, its type and its binder. */
  readonly bound: ReadonlyMap<string, { readonly type: StaticType; readonly binder: Binder }>;
  /** Every value input part the block has: its expected type (null when nothing gives one). */
  readonly inputs: ReadonlyMap<
    string,
    { readonly expects: StaticType | null; readonly fit: SlotFit }
  >;
  /** Every `type` and `symbol_ref` field that has a pattern or `members`. */
  readonly fields: ReadonlyMap<string, SlotFit>;
  /** What each `symbol_decl` field with a pattern declares. */
  readonly declares: ReadonlyMap<string, StaticType>;
  /** The block's value type; null for `any` and `number` outputs, which keep their M2 rules. */
  readonly output: StaticType | null;
  /** A place-typed block: how its place's type fits its output variable. */
  readonly place: SlotFit;
}

/** The most parts of one repeated input (`b2c_model::limits::MAX_VARIADIC_PARTS`). */
export const MAX_VARIADIC_PARTS = 64;

/** `invalid` and `outsideClass`: what the analyser reports and the editor refuses. */
export function isFitError(fit: SlotFit): boolean {
  return fit === 'invalid' || fit === 'outsideClass';
}

/**
 * The start value of a type (slots.md §6): `0`, `0.0`, `true`, `'a'` and `""` for `int`, `double`,
 * `bool`, `char` and `std::string`; `null` for every other type, which has none in M3.
 */
export function startTokens(type: StaticType): readonly TokenJson[] | null {
  if (type.kind !== 'prim') {
    return null;
  }
  switch (type.name) {
    case 'int':
      return [{ num: '0' }];
    case 'double':
      return [{ num: '0.0' }];
    case 'bool':
      return [{ kw: 'true' }];
    case 'char':
      return [{ chr: 'a' }];
    case 'std::string':
      return [{ str: '' }];
    case 'void':
      return null;
  }
}

/** The typing of a block without a signature: nothing bound, checked or declared. */
const UNTYPED: BlockTyping = Object.freeze({
  bound: new Map(),
  inputs: new Map(),
  fields: new Map(),
  declares: new Map(),
  output: null,
  place: 'unknown',
});

/**
 * Types one block (slots.md §2), as `b2c_lang::type_block` does, from its catalog definition (whose
 * signature is built once, `blockSignature`). Pure and deterministic, never throws, linear in the
 * block's parts. A definition whose signature does not read is typed as having none.
 *
 * 1. **Bind from fields**, in catalog order: each `type` or `symbol_ref` field with a pattern.
 * 2. **Bind from value inputs**, in catalog order, a repeated input part by part; an `assign`
 *    input never binds.
 * 3. **Bind from the place**, for a place-typed block.
 * 4. **Resolve and check** every pattern.
 */
export function typeBlock(
  def: SignatureDef,
  parts: BlockParts,
  place: StaticType | null,
  facts: TypeFactsTs,
): BlockTyping {
  const sig = blockSignature(def);
  return sig === null ? UNTYPED : typeBlockSig(sig, parts, place, facts);
}

/** {@link typeBlock} for a signature already built. */
export function typeBlockSig(
  sig: BlockSigTs,
  parts: BlockParts,
  place: StaticType | null,
  facts: TypeFactsTs,
): BlockTyping {
  const env = new TypeEnv(sig);
  const failed = bindParts(sig, parts, facts, env);

  let placeFit: SlotFit = 'unknown';
  if (sig.placeVar !== null && place !== null && env.get(sig.placeVar) === undefined) {
    const fit = env.bindPlace(place, facts);
    if (fit === 'outsideClass') {
      placeFit = 'outsideClass';
    } else if (fit === 'fits' && place.kind !== 'error') {
      placeFit = 'same';
    }
  }

  const resolver = new Resolver(env, parts, failed, facts);
  const { fields, declares } = resolver.fields(sig);
  const output =
    sig.output === null || sig.output.kind === 'any' || sig.output.kind === 'number'
      ? null
      : (resolver.expects(sig.output, null) ?? ERROR_TYPE);
  const bound = new Map<string, { type: StaticType; binder: Binder }>();
  for (const name of [...sig.vars.keys()].sort()) {
    const entry = env.get(name);
    if (entry === undefined) {
      continue;
    }
    const by = entry.by;
    const binder: Binder =
      by === null
        ? { kind: 'place' }
        : 'field' in by
          ? { kind: 'field', name: by.field }
          : { kind: 'input', name: by.input };
    bound.set(name, { type: entry.type, binder });
  }
  return { bound, inputs: resolver.inputs(sig), fields, declares, output, place: placeFit };
}

/**
 * The parts of the value input `base`: the input itself, or each part of a repeated input with its
 * index. An input is repeated when the signature says so or when the parts count parts of it.
 */
export function inputParts(
  sig: BlockSigTs,
  parts: BlockParts,
  base: string,
): (readonly [part: string, index: number | null])[] {
  const count = safeCount(parts, base);
  if (sig.repeated.has(base) || count > 0) {
    return Array.from(
      { length: Math.min(count, MAX_VARIADIC_PARTS) },
      (_, i) => [`${base}${String(i)}`, i] as const,
    );
  }
  return [[base, null]];
}

function safeCount(parts: BlockParts, base: string): number {
  const count = parts.count(base);
  return Number.isSafeInteger(count) && count > 0 ? count : 0;
}

/** Steps 1 and 2; the binding failures by part name. */
function bindParts(
  sig: BlockSigTs,
  parts: BlockParts,
  facts: TypeFactsTs,
  env: TypeEnv,
): Map<string, SlotFit> {
  const failed = new Map<string, SlotFit>();
  const record = (part: string, fit: Fit): void => {
    if (fit === 'outsideClass') {
      failed.set(part, 'outsideClass');
    } else if (fit === 'badShape') {
      failed.set(part, 'invalid');
    }
  };
  for (const part of sig.order) {
    if (!('field' in part)) {
      continue;
    }
    const name = part.field;
    const pattern = sig.fields.get(name);
    if (pattern === undefined || sig.symbolDecls.has(name) || sig.members.has(name)) {
      continue;
    }
    record(name, env.bind(pattern, parts.field(name), { field: name }, facts));
  }
  for (const part of sig.order) {
    if (!('input' in part)) {
      continue;
    }
    const input = sig.inputs.get(part.input);
    const pattern = input?.pattern ?? null;
    if (input === undefined || pattern === null || input.assign) {
      continue;
    }
    for (const [name] of inputParts(sig, parts, part.input)) {
      record(name, env.bind(pattern, parts.input(name), { input: name }, facts));
    }
  }
  return failed;
}

/** The type of a part that has one to check: `null` for every other part, which fits `unknown`. */
function knownType(part: PartType): StaticType | null {
  return part.kind === 'known' && part.type.kind !== 'error' ? part.type : null;
}

/** Step 4: what each part expects and how it fits. */
class Resolver {
  private readonly env: TypeEnv;
  private readonly parts: BlockParts;
  private readonly failed: ReadonlyMap<string, SlotFit>;
  private readonly facts: TypeFactsTs;

  constructor(
    env: TypeEnv,
    parts: BlockParts,
    failed: ReadonlyMap<string, SlotFit>,
    facts: TypeFactsTs,
  ) {
    this.env = env;
    this.parts = parts;
    this.failed = failed;
    this.facts = facts;
  }

  /** A resolved pattern: the type, `error` for `missing`, `null` for `open`. */
  expects(pattern: TypePattern, index: number | null): StaticType | null {
    const r = this.env.resolve(pattern, this.parts, index);
    switch (r.kind) {
      case 'type':
        return r.type;
      case 'missing':
        return ERROR_TYPE;
      case 'open':
        return null;
    }
  }

  inputs(sig: BlockSigTs): Map<string, { expects: StaticType | null; fit: SlotFit }> {
    const inputs = new Map<string, { expects: StaticType | null; fit: SlotFit }>();
    for (const base of [...sig.inputs.keys()].sort()) {
      const input = sig.inputs.get(base);
      if (input === undefined) {
        continue;
      }
      for (const [part, index] of inputParts(sig, this.parts, base)) {
        const expects = input.pattern === null ? null : this.expects(input.pattern, index);
        const type = knownType(this.parts.input(part));
        const fit: SlotFit =
          type === null
            ? 'unknown'
            : (this.failed.get(part) ?? this.inputFit(input, type, expects));
        inputs.set(part, { expects, fit });
      }
    }
    return inputs;
  }

  private inputFit(input: InputSigTs, type: StaticType, expects: StaticType | null): SlotFit {
    const accepts = input.check === null ? null : checkAccepts(input.check, type, this.facts);
    if (accepts === false) {
      return 'outsideClass';
    }
    if (accepts === null || expects === null) {
      return 'unknown';
    }
    return input.assign ? staticAssignment(type, expects) : staticConversion(type, expects);
  }

  fields(sig: BlockSigTs): { fields: Map<string, SlotFit>; declares: Map<string, StaticType> } {
    const fields = new Map<string, SlotFit>();
    const declares = new Map<string, StaticType>();
    for (const name of [...sig.fields.keys()].sort()) {
      const pattern = sig.fields.get(name);
      if (pattern === undefined) {
        continue;
      }
      if (sig.symbolDecls.has(name)) {
        declares.set(name, this.expects(pattern, null) ?? ERROR_TYPE);
        continue;
      }
      if (sig.members.has(name)) {
        continue;
      }
      const type = knownType(this.parts.field(name));
      let fit: SlotFit;
      if (type === null) {
        fit = 'unknown';
      } else {
        const failure = this.failed.get(name);
        if (failure !== undefined) {
          fit = failure;
        } else if (sig.symbolRefs.has(name)) {
          // An earlier part bound the variable to another type: a reference is never converted.
          const expected = this.expects(pattern, null);
          fit =
            expected !== null && expected.kind !== 'error' && !sameType(expected, type)
              ? 'invalid'
              : 'same';
        } else {
          fit = 'same';
        }
      }
      fields.set(name, fit);
    }
    for (const [name, variable] of [...sig.members].sort(([a], [b]) =>
      a < b ? -1 : a > b ? 1 : 0,
    )) {
      const owner = this.env.get(variable);
      let fit: SlotFit = 'unknown';
      if (owner !== undefined && owner.type.kind !== 'error') {
        fit = knownType(this.parts.chosenMember(name, owner.type)) === null ? 'unknown' : 'same';
      }
      fields.set(name, fit);
    }
    return { fields, declares };
  }
}

export type { PartName };
