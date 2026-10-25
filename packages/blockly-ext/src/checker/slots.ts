/**
 * Typed slots on the canvas (ADR-0018, docs/plans/m3-contracts/slots.md §4.3): a block's parts read
 * from Blockly as the analyser would see them, typed with `typeBlock` (../types/slots.ts), and how a
 * candidate would fit an input, which the connection checker refuses when it is a conversion or
 * class error.
 *
 * What only the analysis knows (a symbol's type, whether it is visible at a block, an expression
 * slot's type, the facts about user types) comes from the type oracle, one preview old; what it
 * cannot answer is `unknown`, which binds `error` and fits everything. Nothing here throws into
 * Blockly: an oracle that fails answers nothing.
 *
 * A gesture (a pointer drag, a keyboard move's listing) keeps each block's typing and value type,
 * each candidate's fit and each visibility answer in a {@link SlotCache}; the dragger and the
 * keyboard mover set and clear it ({@link setCurrentSlotCache}). Outside a gesture, a typing is
 * computed when something asks, for one block.
 *
 * A candidate's fit types only what it needs (slots.md §4.3, *Speed*): in an input without a type
 * pattern, the candidate's class alone (`checkAccepts`), as M2's checker did, without typing the
 * parent or asking about any symbol; in a patterned input, the parent's binding parts and that
 * input, not its other inputs.
 */
import type * as Blockly from 'blockly/core';

import { blockDef, partIndex } from './catalog';
import type { BlockDefJson, InputDefJson, TokenJson } from '../generated/catalog';
import { readExprShadow } from '../shadows/state';
import type { SymbolInfo, TypeOracle } from '../services';
import { blockSignature, type BlockSigTs } from '../types/pattern';
import {
  type BlockParts,
  type BlockTyping,
  inputParts,
  isFitError,
  MAX_VARIADIC_PARTS,
  type PartType,
  type SlotFit,
  typeBlockSig,
} from '../types/slots';
import {
  checkAccepts,
  ERROR_TYPE,
  NO_FACTS,
  parseType,
  prim,
  termFromKey,
  typeFromTerm,
  type StaticType,
  type TypeFactsTs,
} from '../types/static-type';
import { checkerTypeOracle } from './oracle';
import { literalType, staticOutputType } from './output-type';

/** How far up the parents a place or a `result` is looked for (project files nest at most 128). */
const MAX_DEPTH = 128;

const EMPTY: PartType = Object.freeze({ kind: 'empty' });
const UNKNOWN: PartType = Object.freeze({ kind: 'unknown' });
const AUTO: PartType = Object.freeze({ kind: 'auto' });
const FROM_PLACE: PartType = Object.freeze({ kind: 'fromPlace' });

function known(type: StaticType): PartType {
  return { kind: 'known', type };
}

/** The M2 output forms the M2 rules type (`output-type.ts`): kept so M2's blocks type as before. */
function hasM2Output(output: string | null): boolean {
  return (
    output === 'any' ||
    output === 'number' ||
    output === 'symbol' ||
    output?.startsWith('field:') === true
  );
}

/** A call into the oracle, guarded: a failure answers `fallback`. */
function ask<T>(call: () => T, fallback: T): T {
  try {
    return call();
  } catch {
    return fallback;
  }
}

/**
 * Each block's typing and value type, each candidate's fit and each visibility answer, for one
 * gesture (slots.md §4.3): the blocks a gesture types do not move while it lasts.
 */
export class SlotCache {
  readonly oracle: TypeOracle;
  /** A block that counts as missing from its input (the block a keyboard move lists places for). */
  readonly absent: Blockly.Block | null;
  /** The facts the cache was made with: a new preview gives new facts, and the cache starts again. */
  private factsSeen: TypeFactsTs;
  private readonly typings = new Map<Blockly.Block, BlockTyping | null>();
  private readonly values = new Map<Blockly.Block, StaticType | 'fromPlace' | null>();
  /** Fits by parent, then input, then candidate. */
  private readonly fits = new Map<Blockly.Block, Map<string, Map<Blockly.Block, SlotFit>>>();
  /** Visibility answers by block, then `symbol` NUL `input`. */
  private readonly visible = new Map<Blockly.Block, Map<string, boolean | null>>();

  constructor(oracle: TypeOracle, options: { readonly absent?: Blockly.Block } = {}) {
    this.oracle = oracle;
    this.absent = options.absent ?? null;
    this.factsSeen = factsOf(oracle);
  }

  /** The facts, starting the cache again when the oracle's facts changed (a new preview). */
  facts(): TypeFactsTs {
    const facts = factsOf(this.oracle);
    if (facts !== this.factsSeen) {
      this.factsSeen = facts;
      this.typings.clear();
      this.values.clear();
      this.fits.clear();
      this.visible.clear();
    }
    return facts;
  }

  /** @internal */
  fit(parent: Blockly.Block, input: string, child: Blockly.Block, make: () => SlotFit): SlotFit {
    this.facts();
    let byInput = this.fits.get(parent);
    if (byInput === undefined) {
      byInput = new Map();
      this.fits.set(parent, byInput);
    }
    let byChild = byInput.get(input);
    if (byChild === undefined) {
      byChild = new Map();
      byInput.set(input, byChild);
    }
    let fit = byChild.get(child);
    if (fit === undefined) {
      fit = make();
      byChild.set(child, fit);
    }
    return fit;
  }

  /** @internal */
  visibleAt(
    symbol: string,
    block: Blockly.Block,
    input: string | null,
    make: () => boolean | null,
  ): boolean | null {
    let byKey = this.visible.get(block);
    if (byKey === undefined) {
      byKey = new Map();
      this.visible.set(block, byKey);
    }
    const key = `${symbol}\u0000${input ?? ''}`;
    if (!byKey.has(key)) {
      byKey.set(key, make());
    }
    return byKey.get(key) ?? null;
  }

  /** @internal */
  typing(block: Blockly.Block, make: () => BlockTyping | null): BlockTyping | null {
    this.facts();
    if (!this.typings.has(block)) {
      this.typings.set(block, make());
    }
    return this.typings.get(block) ?? null;
  }

  /** @internal */
  value(
    block: Blockly.Block,
    make: () => StaticType | 'fromPlace' | null,
  ): StaticType | 'fromPlace' | null {
    this.facts();
    if (!this.values.has(block)) {
      this.values.set(block, make());
    }
    return this.values.get(block) ?? null;
  }
}

/** The oracle's facts, or none. */
function factsOf(oracle: TypeOracle): TypeFactsTs {
  return ask(() => oracle.facts?.() ?? NO_FACTS, NO_FACTS);
}

let currentCache: SlotCache | null = null;

/** The running gesture's cache, which the dragger and the keyboard mover set and clear. */
export function currentSlotCache(): SlotCache | null {
  return currentCache;
}

/** Sets (or with `null` clears) the running gesture's cache. */
export function setCurrentSlotCache(cache: SlotCache | null): void {
  currentCache = cache;
}

/** The context one typing reads. */
interface Context {
  readonly oracle: TypeOracle;
  readonly cache: SlotCache | null;
  readonly facts: TypeFactsTs;
  readonly absent: Blockly.Block | null;
  /** Every reference counts as visible, without asking ({@link candidateRefused}). */
  readonly assumeVisible?: boolean;
}

function context(oracle: TypeOracle, cache: SlotCache | null | undefined): Context {
  const own = cache ?? null;
  const usable = own !== null && own.oracle === oracle ? own : null;
  return {
    oracle,
    cache: usable,
    facts: usable === null ? factsOf(oracle) : usable.facts(),
    absent: usable?.absent ?? null,
  };
}

/** The catalog definition and signature of a block, or `null` outside the catalog. */
function signatureOf(block: Blockly.Block): { def: BlockDefJson; sig: BlockSigTs } | null {
  const def = blockDef(block.type);
  const sig = def === undefined ? null : blockSignature(def);
  return def === undefined || sig === null ? null : { def, sig };
}

/** Whether a block takes its type from its place (its output is a variable no part binds). */
function isPlaceTyped(block: Blockly.Block): boolean {
  return signatureOf(block)?.sig.placeVar !== null && signatureOf(block) !== null;
}

/** A symbol of the latest analysis, or `null`. */
function symbolOf(ctx: Context, id: string | null): SymbolInfo | null {
  if (id === null) {
    return null;
  }
  return ask(() => ctx.oracle.symbol?.(id) ?? null, null);
}

/** Whether the analysis says `id` is visible at `block` (in `input`); `null` when not known. */
function visibleAt(
  ctx: Context,
  id: string,
  block: Blockly.Block,
  input: string | null,
): boolean | null {
  if (ctx.assumeVisible === true) {
    return true;
  }
  const make = (): boolean | null =>
    ask(() => ctx.oracle.visibleAt?.(id, block, input) ?? null, null);
  return ctx.cache === null ? make() : ctx.cache.visibleAt(id, block, input, make);
}

/** A symbol's type as a part (`known`), or `unknown` when it does not read. */
function symbolType(symbol: SymbolInfo | null): PartType {
  if (symbol === null) {
    return UNKNOWN;
  }
  const type = parseType(symbol.kind === 'function' ? symbol.returns : symbol.type);
  return type === null ? UNKNOWN : known(type);
}

/** Whether a symbol is of one of a reference field's kinds (M2's `kinds` filters, 03 §3.6). */
function ofFieldKinds(field: Blockly.Field, symbol: SymbolInfo): boolean {
  const kinds: unknown = (field as { kinds?: unknown }).kinds;
  switch (kinds) {
    case 'functions':
      return symbol.kind === 'function';
    case 'variables':
    case 'assignable':
    case 'numeric':
      return (
        symbol.kind === 'variable' || symbol.kind === 'parameter' || symbol.kind === 'loopVariable'
      );
    default:
      return true;
  }
}

/** The type of tokens as the analyser types a one-token value; `null` when not known. */
function tokensType(tokens: readonly TokenJson[]): StaticType | null {
  const [token] = tokens;
  if (tokens.length !== 1 || token === undefined) {
    return null;
  }
  if ('num' in token) {
    const type = literalType(token.num);
    return type === 'error' ? ERROR_TYPE : prim(type);
  }
  if ('str' in token) {
    return prim('std::string');
  }
  if ('chr' in token) {
    return prim('char');
  }
  if ('kw' in token && (token.kw === 'true' || token.kw === 'false')) {
    return prim('bool');
  }
  return null;
}

/** The definition of an input part (`ITEM2` is part 2 of `ITEM`). */
function inputDefOf(def: BlockDefJson, part: string): InputDefJson | undefined {
  return def.inputs.find((input) =>
    input.repeat === null ? input.name === part : partIndex(part, input.name) !== null,
  );
}

/** The parts of a block on the canvas (slots.md §4.3), with some inputs overridden. */
class CanvasParts implements BlockParts {
  private readonly block: Blockly.Block;
  private readonly def: BlockDefJson;
  private readonly ctx: Context;
  private readonly overrides: ReadonlyMap<string, PartType>;
  private readonly fieldValues: ReadonlyMap<string, unknown>;

  constructor(
    block: Blockly.Block,
    def: BlockDefJson,
    ctx: Context,
    overrides: ReadonlyMap<string, PartType> = new Map(),
    fieldValues: ReadonlyMap<string, unknown> = new Map(),
  ) {
    this.block = block;
    this.def = def;
    this.ctx = ctx;
    this.overrides = overrides;
    this.fieldValues = fieldValues;
  }

  /** A field's value: the one given instead of the field's (a value it had before), or its own. */
  private rawValue(name: string): unknown {
    if (this.fieldValues.has(name)) {
      return this.fieldValues.get(name);
    }
    return this.block.getField(name)?.getValue();
  }

  /** The fields read so far: typing asks for a field when it binds and again when it checks. */
  private readonly fieldsRead = new Map<string, PartType>();

  field(name: string): PartType {
    let part = this.fieldsRead.get(name);
    if (part === undefined) {
      part = this.readField(name);
      this.fieldsRead.set(name, part);
    }
    return part;
  }

  private readField(name: string): PartType {
    const field = this.block.getField(name);
    const definition = this.def.fields.find((candidate) => candidate.name === name);
    if (field === null || definition === undefined) {
      return EMPTY;
    }
    const value = this.rawValue(name);
    switch (definition.kind) {
      case 'type': {
        if (typeof value !== 'string' || value === '') {
          return EMPTY;
        }
        if (value === 'auto') {
          return AUTO;
        }
        const term = termFromKey(value);
        const type = term === null ? null : typeFromTerm(term);
        return type === null ? UNKNOWN : known(type);
      }
      case 'symbol_ref':
        return this.reference(field, refId(value));
      default:
        return UNKNOWN;
    }
  }

  /**
   * A reference: `empty` with nothing chosen; the symbol's type when the analysis says it is
   * visible at the block and of the field's kinds; otherwise `unknown`, what the analyser's
   * `Known(Error)` amounts to.
   */
  private reference(field: Blockly.Field, id: string | null): PartType {
    if (id === null) {
      return EMPTY;
    }
    const symbol = symbolOf(this.ctx, id);
    if (symbol === null || !ofFieldKinds(field, symbol)) {
      return UNKNOWN;
    }
    return visibleAt(this.ctx, id, this.block, null) === true ? symbolType(symbol) : UNKNOWN;
  }

  input(part: string): PartType {
    const override = this.overrides.get(part);
    if (override !== undefined) {
      return override;
    }
    const target = this.block.getInput(part)?.connection?.targetBlock() ?? null;
    if (target === null || target === this.ctx.absent) {
      return this.emptyInput(part);
    }
    if (!target.isShadow()) {
      return blockPart(target, this.ctx);
    }
    return this.shadowPart(part, target);
  }

  /** An empty input: the type of its catalog default tokens, or `empty`. */
  private emptyInput(part: string): PartType {
    const tokens = inputDefOf(this.def, part)?.default ?? [];
    const type = tokensType(tokens);
    return type === null ? EMPTY : known(type);
  }

  /**
   * An expression shadow: one literal is its type; one reference is read as a reference field; any
   * other tokens are the preview's `slotTypes`, else `unknown`.
   */
  private shadowPart(part: string, shadow: Blockly.Block): PartType {
    const value = readExprShadow(shadow);
    if (value === null || value.draft) {
      return UNKNOWN;
    }
    if (value.absent) {
      return this.emptyInput(part);
    }
    const literal = tokensType(value.tokens);
    if (literal !== null) {
      return known(literal);
    }
    const [only] = value.tokens;
    if (value.tokens.length === 1 && only !== undefined && 'ref' in only) {
      const symbol = symbolOf(this.ctx, only.ref);
      if (symbol === null) {
        return UNKNOWN;
      }
      return visibleAt(this.ctx, only.ref, this.block, part) === true
        ? symbolType(symbol)
        : UNKNOWN;
    }
    const slot = ask(() => this.ctx.oracle.slotTypeOf?.(this.block.id, part) ?? null, null);
    return slot === null ? UNKNOWN : known(slot);
  }

  count(base: string): number {
    let count = 0;
    for (const input of this.block.inputList) {
      const index = partIndex(input.name, base);
      if (index !== null) {
        count = Math.max(count, index + 1);
      }
    }
    return count;
  }

  /** `param:F`: parameter `index` of the function chosen in `field`. */
  param(field: string, index: number): PartType {
    const fn = symbolOf(this.ctx, refId(this.rawValue(field)));
    if (fn?.kind !== 'function') {
      return EMPTY;
    }
    const param = fn.params?.[index];
    return param === undefined ? EMPTY : symbolType(symbolOf(this.ctx, param));
  }

  /** `member:F`: field `index` of the struct chosen (or bound) in `field`. */
  member(field: string, index: number): PartType {
    const value = this.rawValue(field);
    const term = typeof value === 'string' && value.startsWith('{') ? termFromKey(value) : null;
    const owner = symbolOf(this.ctx, term?.kind === 'ref' ? term.id : refId(value));
    if (owner?.kind !== 'struct') {
      return EMPTY;
    }
    const member = owner.fields?.[index];
    return member === undefined ? EMPTY : symbolType(symbolOf(this.ctx, member));
  }

  /** A `members` field: the chosen member's type when the preview lists it as one of `owner`'s. */
  chosenMember(field: string, owner: StaticType): PartType {
    const member = symbolOf(this.ctx, refId(this.rawValue(field)));
    if (member === null || owner.kind !== 'named') {
      return UNKNOWN;
    }
    const isMember =
      (member.kind === 'field' || member.kind === 'enumerator') && member.owner === owner.id;
    return isMember ? symbolType(member) : UNKNOWN;
  }

  /**
   * `result`: up the parents to the root: `int` under *when program starts*, the `RETURNS` type
   * under *define* (`empty` for `void`), `unknown` loose on the canvas or in the toolbox.
   */
  result(): PartType {
    if (this.block.isInFlyout) {
      return UNKNOWN;
    }
    const block = this.block.getRootBlock();
    if (block.type === 'program.main') {
      return known(prim('int'));
    }
    if (block.type === 'func.define') {
      const value: unknown = block.getField('RETURNS')?.getValue();
      if (value === 'void') {
        return EMPTY;
      }
      const term = typeof value === 'string' ? termFromKey(value) : null;
      const type = term === null ? null : typeFromTerm(term);
      return type === null ? UNKNOWN : known(type);
    }
    return UNKNOWN;
  }
}

/** The symbol ID a reference field holds: its value (an ID), or `{ref}` (a field holding the project value). */
function refId(value: unknown): string | null {
  if (typeof value === 'string') {
    return value === '' ? null : value;
  }
  if (typeof value === 'object' && value !== null && 'ref' in value) {
    const ref: unknown = value.ref;
    return typeof ref === 'string' ? ref : null;
  }
  return null;
}

/** A block in an input, as a part: `empty` when disabled, `fromPlace` when place-typed, else its value type. */
function blockPart(block: Blockly.Block, ctx: Context): PartType {
  if (!block.isEnabled()) {
    return EMPTY;
  }
  const value = valueTypeIn(block, ctx, 0);
  if (value === 'fromPlace') {
    return FROM_PLACE;
  }
  return value === null ? UNKNOWN : known(value);
}

/**
 * The typing of a block from its parts now, with some inputs overridden; with `only`, of that
 * input part alone (the others are left out of `inputs`, and not read unless they bind).
 */
function typeWith(
  block: Blockly.Block,
  ctx: Context,
  overrides: ReadonlyMap<string, PartType>,
  place: StaticType | null,
  only?: string,
): BlockTyping | null {
  const signature = signatureOf(block);
  if (signature === null) {
    return null;
  }
  const parts = new CanvasParts(block, signature.def, ctx, overrides);
  return ask(() => typeBlockSig(signature.sig, parts, place, ctx.facts, only), null);
}

/** The value type of a block: its signature's output, M2's rules, or the oracle. */
function valueTypeIn(
  block: Blockly.Block,
  ctx: Context,
  depth: number,
): StaticType | 'fromPlace' | null {
  const make = (): StaticType | 'fromPlace' | null => {
    const signature = signatureOf(block);
    if (signature === null) {
      return ask(() => ctx.oracle.outputTypeOf(block), null);
    }
    if (signature.sig.placeVar !== null) {
      return 'fromPlace';
    }
    const { def } = signature;
    if (def.output === null) {
      return null;
    }
    if (hasM2Output(def.output) || depth > MAX_DEPTH) {
      return ask(() => staticOutputType(block, def, ctx.oracle), null);
    }
    return typeWith(block, ctx, new Map(), placeOf(block, ctx, depth + 1))?.output ?? null;
  };
  return ctx.cache === null ? make() : ctx.cache.value(block, make);
}

/**
 * The type a place-typed block's place expects: the `expects` of the input it is in, in its
 * parent's typing (the parent's own place found the same way, up the chain). `null` when not known.
 */
function placeOf(block: Blockly.Block, ctx: Context, depth: number): StaticType | null {
  const parent = block.getParent();
  if (parent === null || depth > MAX_DEPTH || !isPlaceTyped(block)) {
    return null;
  }
  const input = parent.getInputWithBlock(block)?.name;
  if (input === undefined) {
    return null;
  }
  const parentPlace = isPlaceTyped(parent) ? placeOf(parent, ctx, depth + 1) : null;
  const typing = typeWith(parent, ctx, new Map(), parentPlace);
  return typing?.inputs.get(input)?.expects ?? null;
}

/** A block's typing from its parts on the canvas now; `null` for a block outside the catalog. */
export function blockTyping(
  block: Blockly.Block,
  oracle: TypeOracle,
  cache?: SlotCache | null,
): BlockTyping | null {
  const ctx = context(oracle, cache);
  const make = (): BlockTyping | null =>
    typeWith(block, ctx, new Map(), isPlaceTyped(block) ? placeOf(block, ctx, 0) : null);
  return ctx.cache === null ? make() : ctx.cache.typing(block, make);
}

/** The block's value type, `fromPlace` for a place-typed block, or `null` when not known. */
export function valueType(
  block: Blockly.Block,
  oracle: TypeOracle,
  cache?: SlotCache | null,
): StaticType | 'fromPlace' | null {
  return valueTypeIn(block, context(oracle, cache), 0);
}

/**
 * How `child` would fit in `parent`'s value input `input`, typed as if it were there (slots.md
 * §4.3): a place-typed child is typed with the place the parent's typing gives the input, and the
 * answer is its `place`; any other child is the input's fit with the child's value type in it.
 * `unknown` for a parent outside the catalog or an input it does not type.
 */
export function candidateFit(
  parent: Blockly.Block,
  input: string,
  child: Blockly.Block,
  oracle: TypeOracle,
  cache?: SlotCache | null,
): SlotFit {
  const ctx = context(oracle, cache);
  const make = (): SlotFit => fitIn(parent, input, child, ctx);
  return ctx.cache === null ? make() : ctx.cache.fit(parent, input, child, make);
}

/**
 * Whether the connection checker refuses `child` in `parent`'s value input `input`: whether
 * {@link candidateFit} is a fit error ({@link isFitError}), worked out with fewer scope queries.
 * The fit is first worked out as if every reference were visible where it is, which asks nothing;
 * only when that refuses is it worked out again with the analysis's answers. That gives the same
 * answer: a reference that is not visible is `unknown`, which binds `error` and fits everything,
 * so it can only turn a refusal into a fit, never a fit into a refusal.
 */
export function candidateRefused(
  parent: Blockly.Block,
  input: string,
  child: Blockly.Block,
  oracle: TypeOracle,
  cache?: SlotCache | null,
): boolean {
  const ctx = context(oracle, cache);
  const make = (): SlotFit => {
    // Without the cache: what it keeps holds the analysis's answers.
    const optimistic = fitIn(parent, input, child, { ...ctx, cache: null, assumeVisible: true });
    return isFitError(optimistic) ? fitIn(parent, input, child, ctx) : optimistic;
  };
  const fit =
    ctx.cache === null ? make() : ctx.cache.fit(parent, `${input}\u0000refused`, child, make);
  return isFitError(fit);
}

/** {@link candidateFit} without the cache of fits. */
function fitIn(parent: Blockly.Block, input: string, child: Blockly.Block, ctx: Context): SlotFit {
  const placeTyped = isPlaceTyped(child);
  if (!placeTyped) {
    const plain = patternlessFit(parent, input, child, ctx);
    if (plain !== null) {
      return plain;
    }
  }
  const parentPlace = isPlaceTyped(parent) ? placeOf(parent, ctx, 0) : null;
  if (placeTyped) {
    const typing = typeWith(parent, ctx, new Map([[input, FROM_PLACE]]), parentPlace, input);
    const expects = typing?.inputs.get(input)?.expects ?? null;
    const own = typeWith(child, ctx, new Map(), expects);
    return own?.place ?? 'unknown';
  }
  const part = child.isEnabled() ? blockPart(child, ctx) : EMPTY;
  const typing = typeWith(parent, ctx, new Map([[input, part]]), parentPlace, input);
  return typing?.inputs.get(input)?.fit ?? 'unknown';
}

/**
 * The fit of a candidate in an input part without a type pattern, which is what typing the parent
 * would give without typing it: nothing binds or resolves there, so the fit is `outsideClass` when
 * the input's class refuses the candidate's type and `unknown` otherwise. `null` when the input
 * has a pattern, or is not a part the parent's typing lists, so the parent is typed.
 */
function patternlessFit(
  parent: Blockly.Block,
  input: string,
  child: Blockly.Block,
  ctx: Context,
): SlotFit | null {
  const signature = signatureOf(parent);
  if (signature === null) {
    return 'unknown';
  }
  const definition = inputDefOf(signature.def, input);
  const sig = definition === undefined ? undefined : signature.sig.inputs.get(definition.name);
  if (definition === undefined || sig?.pattern !== null) {
    return null;
  }
  if (definition.repeat !== null || signature.sig.repeated.has(definition.name)) {
    // Typing lists the parts the block has, up to the most a repeated input may have.
    const index = partIndex(input, definition.name);
    if (index === null || index >= MAX_VARIADIC_PARTS || parent.getInput(input) === null) {
      return null;
    }
  }
  const part = child.isEnabled() ? blockPart(child, ctx) : EMPTY;
  const type = part.kind === 'known' && part.type.kind !== 'error' ? part.type : null;
  if (type === null || sig.check === null) {
    return 'unknown';
  }
  return checkAccepts(sig.check, type, ctx.facts) === false ? 'outsideClass' : 'unknown';
}

/**
 * Starts a gesture's cache with the checker's oracle (the dragger when a drag starts, the keyboard
 * mover while it lists places, with `absent` set to the moved block). Returns the cache, which
 * {@link endSlotGesture} drops.
 */
export function beginSlotGesture(options: { readonly absent?: Blockly.Block } = {}): SlotCache {
  const cache = new SlotCache(checkerTypeOracle(), options);
  setCurrentSlotCache(cache);
  return cache;
}

/** Ends the running gesture's cache. */
export function endSlotGesture(): void {
  setCurrentSlotCache(null);
}

/**
 * What each value input part of `block` expects, for its start value (slots.md §6): each part is
 * typed with itself empty, so a value never sets its own start value (`create [auto]`), and with
 * `field` holding `value` instead of its own when given (the value before a change). References
 * count whatever their scope, as M2's start values did. `null` for a part nothing gives a type;
 * the map is empty for a block outside the catalog.
 */
export function startValueExpectations(
  block: Blockly.Block,
  field?: { readonly name: string; readonly value: unknown },
): ReadonlyMap<string, StaticType | null> {
  const signature = signatureOf(block);
  const out = new Map<string, StaticType | null>();
  if (signature === null) {
    return out;
  }
  const base = checkerTypeOracle();
  const oracle: TypeOracle = {
    outputTypeOf: (asked) => base.outputTypeOf(asked),
    slotTypeOf: (id, input) => base.slotTypeOf?.(id, input) ?? null,
    symbol: (id) => base.symbol?.(id) ?? null,
    visibleAt: () => true,
    facts: () => base.facts?.() ?? NO_FACTS,
  };
  const ctx = context(oracle, null);
  const values = new Map<string, unknown>(field === undefined ? [] : [[field.name, field.value]]);
  const reader = new CanvasParts(block, signature.def, ctx, new Map(), values);
  for (const input of signature.sig.inputs.keys()) {
    for (const [part] of inputParts(signature.sig, reader, input)) {
      const parts = new CanvasParts(block, signature.def, ctx, new Map([[part, EMPTY]]), values);
      const typing = ask(
        () => typeBlockSig(signature.sig, parts, placeOf(block, ctx, 0), ctx.facts),
        null,
      );
      out.set(part, typing?.inputs.get(part)?.expects ?? null);
    }
  }
  return out;
}
