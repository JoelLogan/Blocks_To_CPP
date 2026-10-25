/**
 * Catalog signatures in the editor (types.md §4, mirroring `b2c_ir::sig`): type patterns, read from
 * their text (a block's `output`, as catalog files write it) or their tagged JSON (a part's `type`,
 * as `catalog.json` writes it), a block's signature built from its catalog definition the way
 * `b2c-catalog`'s `build()` builds it, and {@link TypeEnv}, which binds a block's type variables
 * from its parts and resolves its patterns.
 *
 * Nothing here throws: a pattern or a signature that does not read is `null`, and a block without
 * a signature keeps the M2 rules.
 */
import type { TypePatternJson } from '../generated/catalog';
import {
  applyTypeFn,
  ERROR_TYPE,
  inClass,
  isTypeClass,
  libOf,
  libType,
  parseType,
  sameType,
  type StaticType,
  type TypeClass,
  type TypeFactsTs,
  type TypeFn,
  typeFromTerm,
  parseTerm,
} from './static-type';
import type { BlockParts, PartType } from './slots';

/** A type pattern (`b2c_ir::sig::TypePattern`, types.md §4.2). */
export type TypePattern =
  /** `any` (M2): the analyser decides. */
  | { readonly kind: 'any' }
  /** `number` (M2): `int` or `double` by the operands; outputs only. */
  | { readonly kind: 'number' }
  /** A fixed type, also a template whose arguments are all fixed (`std::vector<int>`). */
  | { readonly kind: 'type'; readonly type: StaticType }
  /** A type variable: `T`, `K`, `T2`. */
  | { readonly kind: 'var'; readonly name: string }
  /** A library template with at least one argument that is not a fixed type: `std::vector<T>`. */
  | { readonly kind: 'lib'; readonly name: string; readonly args: readonly TypePattern[] }
  /** A type function: `elem(C)`. */
  | { readonly kind: 'fn'; readonly fn: TypeFn; readonly of: TypePattern }
  /**
   * `field:NAME` (M2): the type chosen in the type field `NAME`. On a repeated `symbol_decl` field
   * whose `NAME` is a repeated type field with the same count, the type chosen in `NAME`'s part of
   * the same index (docs/plans/m3-contracts/w2.md §2.1: a catch part's variable takes its type
   * from that part's error type).
   */
  | { readonly kind: 'field'; readonly field: string }
  /** `symbol` (M2, `field` null until the signature names it) or `symbol:NAME`. */
  | { readonly kind: 'symbol'; readonly field: string | null }
  /** `param:NAME`: on a repeated input, parameter `i` of the function in field `NAME`. */
  | { readonly kind: 'param'; readonly field: string }
  /** `member:NAME`: on a repeated input, field `i` of the struct in field `NAME`. */
  | { readonly kind: 'member'; readonly field: string }
  /** `result`: what a `return` gives back where the block is. */
  | { readonly kind: 'result' };

/** A pattern as `catalog.json` writes it on a part (types.md §4.5): JSON tagged by `kind`. */
export type { TypePatternJson };

/** The longest pattern text, in bytes (`TypePattern::MAX_LEN`). */
export const MAX_PATTERN_LEN = 128;
/** The deepest nesting of templates and type functions (`TypePattern::MAX_DEPTH`). */
export const MAX_PATTERN_DEPTH = 8;

const TYPE_FNS: readonly TypeFn[] = ['elem', 'index', 'key'];

/** A type variable's name: an upper-case letter, optionally followed by a digit. */
export function isTypeVar(name: string): boolean {
  return /^[A-Z][0-9]?$/.test(name);
}

/** A field or input name in a pattern: `[A-Za-z_][A-Za-z0-9_]*`, 1 to 64 characters. */
function isPartName(name: string): boolean {
  return name.length <= 64 && /^[A-Za-z_][A-Za-z0-9_]*$/.test(name);
}

/** Whether a pattern is a fixed type, which one catalog `default` can fit. */
function isFixed(
  pattern: TypePattern,
): pattern is { readonly kind: 'type'; readonly type: StaticType } {
  return pattern.kind === 'type';
}

/** A template pattern: a fixed type when every argument is one (`template` in sig.rs). */
function template(name: string, args: readonly TypePattern[]): TypePattern | null {
  const info = libType(name);
  if (args.length !== info?.arity) {
    return null;
  }
  if (args.every(isFixed)) {
    const type = libOf(
      name,
      args.map((arg) => (arg as { readonly type: StaticType }).type),
    );
    return type === null ? null : { kind: 'type', type };
  }
  // A fixed argument must still be a value type.
  const bad = args.some((arg) => arg.kind === 'type' && libOf('std::vector', [arg.type]) === null);
  return bad ? null : { kind: 'lib', name, args };
}

/**
 * Parses a pattern's text (`TypePattern::parse`, types.md §4.2): at most {@link MAX_PATTERN_LEN}
 * bytes and {@link MAX_PATTERN_DEPTH} levels; `null` outside the grammar, for an unknown type, or
 * for a library type with the wrong arguments.
 */
export function parsePatternText(text: string): TypePattern | null {
  if (text.length > MAX_PATTERN_LEN || !/^[\x20-\x7e]*$/.test(text)) {
    return null;
  }
  let at = 0;
  const spaces = (): void => {
    while (text.charAt(at) === ' ') {
      at += 1;
    }
  };
  const eat = (char: string): boolean => {
    spaces();
    if (text.charAt(at) === char) {
      at += 1;
      return true;
    }
    return false;
  };
  const word = (): string => {
    spaces();
    const start = at;
    while (at < text.length && /[A-Za-z0-9_:]/.test(text.charAt(at))) {
      at += 1;
    }
    return text.slice(start, at);
  };
  const pattern = (depth: number): TypePattern | null => {
    const w = word();
    const colon = w.indexOf(':');
    if (!w.includes('::') && colon !== -1) {
      const form = w.slice(0, colon);
      const name = w.slice(colon + 1);
      if (!isPartName(name)) {
        return null;
      }
      switch (form) {
        case 'field':
        case 'param':
        case 'member':
          return { kind: form, field: name };
        case 'symbol':
          return { kind: 'symbol', field: name };
        default:
          return null;
      }
    }
    switch (w) {
      case '':
        return null;
      case 'any':
      case 'number':
      case 'result':
        return { kind: w };
      case 'symbol':
        return { kind: 'symbol', field: null };
    }
    if (isTypeVar(w)) {
      return { kind: 'var', name: w };
    }
    const fn = TYPE_FNS.find((candidate) => candidate === w);
    if (fn !== undefined && eat('(')) {
      if (depth >= MAX_PATTERN_DEPTH) {
        return null;
      }
      const of = pattern(depth + 1);
      return of !== null && eat(')') ? { kind: 'fn', fn, of } : null;
    }
    if (eat('<')) {
      if (depth >= MAX_PATTERN_DEPTH) {
        return null;
      }
      const args: TypePattern[] = [];
      do {
        const arg = pattern(depth + 1);
        if (arg === null) {
          return null;
        }
        args.push(arg);
      } while (eat(','));
      return eat('>') ? template(w, args) : null;
    }
    const term = parseTerm(w);
    const type = term === null ? null : typeFromTerm(term);
    return type === null ? null : { kind: 'type', type };
  };
  const result = pattern(0);
  spaces();
  return result !== null && at === text.length ? result : null;
}

/** Whether a value is a plain JSON object. */
function isObject(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Reads a pattern in its tagged JSON (types.md §4.5), with the same checks as its text: `null` for
 * an unknown kind, a variable or part name that breaks its rule, an unknown type, a template with
 * the wrong arguments, or more than {@link MAX_PATTERN_DEPTH} levels.
 */
export function parsePatternJson(json: unknown, depth = 0): TypePattern | null {
  if (!isObject(json) || depth > MAX_PATTERN_DEPTH) {
    return null;
  }
  const field = json['field'];
  switch (json['kind']) {
    case 'any':
    case 'number':
    case 'result':
      return { kind: json['kind'] };
    case 'type': {
      const type = parseType(json['type']);
      return type === null || type.kind === 'error' ? null : { kind: 'type', type };
    }
    case 'var': {
      const name = json['name'];
      return typeof name === 'string' && isTypeVar(name) ? { kind: 'var', name } : null;
    }
    case 'lib': {
      const name = json['name'];
      const args = json['args'];
      if (typeof name !== 'string' || !Array.isArray(args) || depth >= MAX_PATTERN_DEPTH) {
        return null;
      }
      const parsed: TypePattern[] = [];
      for (const arg of args) {
        const pattern = parsePatternJson(arg, depth + 1);
        if (pattern === null) {
          return null;
        }
        parsed.push(pattern);
      }
      return template(name, parsed);
    }
    case 'fn': {
      const fn = TYPE_FNS.find((candidate) => candidate === json['fn']);
      const of = depth >= MAX_PATTERN_DEPTH ? null : parsePatternJson(json['of'], depth + 1);
      return fn === undefined || of === null ? null : { kind: 'fn', fn, of };
    }
    case 'symbol':
      if (field === null || field === undefined) {
        return { kind: 'symbol', field: null };
      }
      return typeof field === 'string' && isPartName(field) ? { kind: 'symbol', field } : null;
    case 'field':
    case 'param':
    case 'member':
      return typeof field === 'string' && isPartName(field) ? { kind: json['kind'], field } : null;
    default:
      return null;
  }
}

/**
 * Every type variable of a pattern (with repeats); with `inFunctions` false, only those outside
 * every type function, each once: the ones a part with this pattern can bind (rule 1).
 */
export function patternVars(pattern: TypePattern, inFunctions = true): string[] {
  const vars: string[] = [];
  const walk = (p: TypePattern): void => {
    switch (p.kind) {
      case 'var':
        vars.push(p.name);
        break;
      case 'lib':
        p.args.forEach(walk);
        break;
      case 'fn':
        if (inFunctions) {
          walk(p.of);
        }
        break;
      default:
        break;
    }
  };
  walk(pattern);
  return inFunctions ? vars : [...new Set(vars)];
}

// --- Signatures -------------------------------------------------------------------------------

/** A part of a block that has a pattern (`b2c_ir::sig::PartName`). */
export type PartName = { readonly field: string } | { readonly input: string };

/** What a value input expects (`b2c_ir::sig::InputSig`). */
export interface InputSigTs {
  /** Its `check` class; `null` for a class this editor does not know (membership not known). */
  readonly check: TypeClass | null;
  readonly pattern: TypePattern | null;
  /** `assign = true`: assigned, not initialised (`char` fits `std::string`); never binds. */
  readonly assign: boolean;
}

/** A block's signature (`b2c_ir::sig::BlockSig`), as typing reads it. */
export interface BlockSigTs {
  readonly vars: ReadonlyMap<string, TypeClass>;
  readonly output: TypePattern | null;
  readonly fields: ReadonlyMap<string, TypePattern>;
  /** Every value input, a repeated input under its base name. */
  readonly inputs: ReadonlyMap<string, InputSigTs>;
  readonly order: readonly PartName[];
  readonly placeVar: string | null;
  readonly members: ReadonlyMap<string, string>;
  readonly symbolRefs: ReadonlySet<string>;
  readonly symbolDecls: ReadonlySet<string>;
  /** The inputs with a `repeat`, by their base names. */
  readonly repeated: ReadonlySet<string>;
  /**
   * The fields with a `repeat` (w2.md §2.1, `repeated_fields` in sig.rs), with or without a
   * pattern: `field:F` on a repeated field reads `F`'s part of the same index, and a repeated
   * `symbol_decl` declares one symbol per part.
   */
  readonly repeatedFields: ReadonlySet<string>;
}

/**
 * The keys of a catalog definition a signature is built from: the generated `BlockDefJson` has
 * them all. `output` is the pattern's text, as catalog files write it (a test may give it tagged).
 */
export interface SignatureDef {
  readonly output: string | TypePatternJson | null;
  readonly typeVars: Readonly<Record<string, string>>;
  readonly order: readonly PartName[];
  readonly placeVar: string | null;
  readonly members: Readonly<Record<string, string>>;
  readonly fields: readonly {
    readonly name: string;
    readonly kind: string;
    readonly type: TypePatternJson | null;
    /** A repeated field (w2.md §2.1); absent or `null` for one shown once. */
    readonly repeat?: unknown;
  }[];
  readonly inputs: readonly {
    readonly name: string;
    readonly check: string;
    readonly type: TypePatternJson | null;
    readonly assign: boolean;
    readonly repeat: unknown;
  }[];
}

/** M2's `symbol` named after the block's one `symbol_ref` field (`name_symbol` in signature.rs). */
function nameSymbol(pattern: TypePattern, field: string | null): TypePattern {
  switch (pattern.kind) {
    case 'symbol':
      return pattern.field === null ? { kind: 'symbol', field } : pattern;
    case 'lib':
      return {
        kind: 'lib',
        name: pattern.name,
        args: pattern.args.map((arg) => nameSymbol(arg, field)),
      };
    case 'fn':
      return { kind: 'fn', fn: pattern.fn, of: nameSymbol(pattern.of, field) };
    default:
      return pattern;
  }
}

const SIGNATURES = new WeakMap<SignatureDef, BlockSigTs | null>();

/**
 * The signature of a catalog definition, built as `b2c-catalog`'s `build()` builds it: `symbolRefs`
 * and `symbolDecls` are the `symbol_ref` and `symbol_decl` fields that have a pattern, `repeated`
 * and `repeatedFields` the inputs and the fields with a `repeat`, and M2's `symbol` names the
 * block's one `symbol_ref` field. Kept per
 * definition object. `null` when a pattern or a class does not read (the catalog checked them, so
 * only a damaged definition gets here): the block then keeps the M2 rules.
 */
export function blockSignature(def: SignatureDef): BlockSigTs | null {
  if (SIGNATURES.has(def)) {
    return SIGNATURES.get(def) ?? null;
  }
  const sig = buildSignature(def);
  SIGNATURES.set(def, sig);
  return sig;
}

function buildSignature(def: SignatureDef): BlockSigTs | null {
  const refs = def.fields.filter((field) => field.kind === 'symbol_ref');
  const [onlyRef] = refs;
  const symbolField = refs.length === 1 && onlyRef !== undefined ? onlyRef.name : null;
  const vars = new Map<string, TypeClass>();
  for (const [name, cls] of Object.entries(def.typeVars)) {
    if (!isTypeVar(name) || !isTypeClass(cls)) {
      return null;
    }
    vars.set(name, cls);
  }
  let output: TypePattern | null = null;
  if (def.output !== null) {
    const parsed =
      typeof def.output === 'string' ? parsePatternText(def.output) : parsePatternJson(def.output);
    if (parsed === null) {
      return null;
    }
    output = nameSymbol(parsed, symbolField);
  }
  const fields = new Map<string, TypePattern>();
  const symbolRefs = new Set<string>();
  const symbolDecls = new Set<string>();
  const repeatedFields = new Set<string>();
  for (const field of def.fields) {
    if (field.repeat !== null && field.repeat !== undefined) {
      repeatedFields.add(field.name);
    }
    if (field.type === null) {
      continue;
    }
    const parsed = parsePatternJson(field.type);
    if (parsed === null) {
      return null;
    }
    fields.set(field.name, nameSymbol(parsed, symbolField));
    if (field.kind === 'symbol_ref') {
      symbolRefs.add(field.name);
    } else if (field.kind === 'symbol_decl') {
      symbolDecls.add(field.name);
    }
  }
  const inputs = new Map<string, InputSigTs>();
  const repeated = new Set<string>();
  for (const input of def.inputs) {
    let pattern: TypePattern | null = null;
    if (input.type !== null) {
      const parsed = parsePatternJson(input.type);
      if (parsed === null) {
        return null;
      }
      pattern = nameSymbol(parsed, symbolField);
    }
    inputs.set(input.name, {
      check: isTypeClass(input.check) ? input.check : null,
      pattern,
      assign: input.assign,
    });
    if (input.repeat !== null && input.repeat !== undefined) {
      repeated.add(input.name);
    }
  }
  const members = new Map<string, string>();
  for (const [field, name] of Object.entries(def.members)) {
    if (!isTypeVar(name)) {
      return null;
    }
    members.set(field, name);
  }
  const placeVar = def.placeVar !== null && isTypeVar(def.placeVar) ? def.placeVar : null;
  return {
    vars,
    output,
    fields,
    inputs,
    order: def.order,
    placeVar,
    members,
    symbolRefs,
    symbolDecls,
    repeated,
    repeatedFields,
  };
}

// --- Binding and resolving --------------------------------------------------------------------

/** How a binding went (`b2c_ir::sig::Fit`), from the best to the worst. */
export type Fit = 'fits' | 'unknown' | 'outsideClass' | 'badShape';

const FIT_ORDER: readonly Fit[] = ['fits', 'unknown', 'outsideClass', 'badShape'];

function worse(a: Fit, b: Fit): Fit {
  return FIT_ORDER.indexOf(a) >= FIT_ORDER.indexOf(b) ? a : b;
}

/** A resolved pattern (`b2c_ir::sig::Resolution`). */
export type Resolution =
  | { readonly kind: 'type'; readonly type: StaticType }
  /** Something it depends on is empty, unknown or already wrong: `error` fits. */
  | { readonly kind: 'missing' }
  /** Nothing could give it a type (no pattern, or only `auto` and place-typed values). */
  | { readonly kind: 'open' };

const MISSING: Resolution = Object.freeze({ kind: 'missing' });
const OPEN: Resolution = Object.freeze({ kind: 'open' });

function resolved(type: StaticType): Resolution {
  return { kind: 'type', type };
}

/**
 * The type variables of one block being typed (`b2c_ir::sig::TypeEnv`): their classes, what is
 * bound and by which part (types.md §4.3, rules 1 to 3).
 */
export class TypeEnv {
  private readonly classes: ReadonlyMap<string, TypeClass>;
  private readonly placeVar: string | null;
  private readonly members: ReadonlyMap<string, string>;
  /** The repeated fields, whose parts `field:` reads by index. */
  private readonly repeatedFields: ReadonlySet<string>;
  /** Each bound variable, its type and its binder (`null`: the place). */
  private readonly bound = new Map<string, { type: StaticType; by: PartName | null }>();
  /** Unbound variables that an empty part could have bound. */
  private readonly empty = new Set<string>();

  constructor(
    sig: Pick<BlockSigTs, 'vars' | 'placeVar' | 'members'> &
      Partial<Pick<BlockSigTs, 'repeatedFields'>>,
  ) {
    this.classes = sig.vars;
    this.placeVar = sig.placeVar;
    this.members = sig.members;
    this.repeatedFields = sig.repeatedFields ?? new Set();
  }

  /**
   * Unifies `pattern` with `part` (rules 1 and 2) and binds the pattern's unbound variables that
   * are outside every type function: the first binding wins. A failed binding, an unknown part and
   * unknown membership bind them to `error`; `empty`, `auto` and `fromPlace` bind nothing.
   */
  bind(pattern: TypePattern, part: PartType, by: PartName, facts: TypeFactsTs): Fit {
    switch (part.kind) {
      case 'empty':
        for (const name of patternVars(pattern, false)) {
          if (!this.bound.has(name)) {
            this.empty.add(name);
          }
        }
        return 'fits';
      case 'auto':
      case 'fromPlace':
        return 'fits';
      case 'unknown':
        this.bindError(pattern, by);
        return 'unknown';
      case 'known':
        break;
    }
    if (part.type.kind === 'error') {
      this.bindError(pattern, by);
      return 'fits';
    }
    const found = new Map<string, StaticType>();
    const fit = this.unify(pattern, part.type, facts, found);
    if (fit === 'fits') {
      for (const [name, type] of found) {
        if (!this.bound.has(name)) {
          this.bound.set(name, { type, by });
        }
      }
    } else {
      this.bindError(pattern, by);
    }
    return fit;
  }

  /** Binds the place-typed block's variable to its place's type (rule 3). */
  bindPlace(place: StaticType | null, facts: TypeFactsTs): Fit {
    const name = this.placeVar;
    if (name === null || place === null || this.bound.has(name)) {
      return 'fits';
    }
    const member = inClass(place, this.classOf(name), facts);
    if (member === true) {
      this.bound.set(name, { type: place, by: null });
      return 'fits';
    }
    this.bound.set(name, { type: ERROR_TYPE, by: null });
    return member === false ? 'outsideClass' : 'unknown';
  }

  /**
   * Resolves a pattern against what is bound; `index` is the part's index for `param:`, `member:`
   * and a `field:` that names a repeated field.
   */
  resolve(pattern: TypePattern, parts: BlockParts, index: number | null): Resolution {
    const known = (part: PartType): Resolution =>
      part.kind === 'known' ? resolved(part.type) : MISSING;
    switch (pattern.kind) {
      case 'any':
      case 'number':
        return OPEN;
      case 'type':
        return resolved(pattern.type);
      case 'var': {
        const bound = this.bound.get(pattern.name);
        if (bound !== undefined) {
          return resolved(bound.type);
        }
        return this.empty.has(pattern.name) ? MISSING : OPEN;
      }
      case 'lib': {
        const types: StaticType[] = [];
        let missing = false;
        let open = false;
        for (const arg of pattern.args) {
          const r = this.resolve(arg, parts, index);
          if (r.kind === 'type') {
            types.push(r.type);
          } else if (r.kind === 'missing') {
            missing = true;
          } else {
            open = true;
          }
        }
        if (missing) {
          return MISSING;
        }
        if (open) {
          return OPEN;
        }
        const type = libOf(pattern.name, types);
        return type === null ? MISSING : resolved(type);
      }
      case 'fn': {
        const r = this.resolve(pattern.of, parts, index);
        if (r.kind !== 'type') {
          return r;
        }
        const type = applyTypeFn(pattern.fn, r.type);
        return type === null ? MISSING : resolved(type);
      }
      case 'field': {
        if (!this.repeatedFields.has(pattern.field)) {
          return known(parts.field(pattern.field));
        }
        // A repeated field's part of the same index (w2.md §2.1).
        return index === null ? MISSING : known(parts.field(`${pattern.field}${String(index)}`));
      }
      case 'symbol': {
        if (pattern.field === null) {
          return MISSING;
        }
        const name = this.members.get(pattern.field);
        if (name === undefined) {
          return known(parts.field(pattern.field));
        }
        const owner = this.bound.get(name);
        return owner === undefined || owner.type.kind === 'error'
          ? MISSING
          : known(parts.chosenMember(pattern.field, owner.type));
      }
      case 'param':
        return index === null ? MISSING : known(parts.param(pattern.field, index));
      case 'member':
        return index === null ? MISSING : known(parts.member(pattern.field, index));
      case 'result':
        return known(parts.result());
    }
  }

  /** A bound variable's type and its binder (`null` when the place bound it). */
  get(name: string): { readonly type: StaticType; readonly by: PartName | null } | undefined {
    return this.bound.get(name);
  }

  private classOf(name: string): TypeClass {
    return this.classes.get(name) ?? 'any';
  }

  private bindError(pattern: TypePattern, by: PartName): void {
    for (const name of patternVars(pattern, false)) {
      if (!this.bound.has(name)) {
        this.bound.set(name, { type: ERROR_TYPE, by });
      }
    }
  }

  private unify(
    pattern: TypePattern,
    type: StaticType,
    facts: TypeFactsTs,
    found: Map<string, StaticType>,
  ): Fit {
    switch (pattern.kind) {
      case 'var': {
        if (this.bound.has(pattern.name)) {
          return 'fits';
        }
        const earlier = found.get(pattern.name);
        if (earlier !== undefined) {
          return sameType(earlier, type) ? 'fits' : 'badShape';
        }
        const member = inClass(type, this.classOf(pattern.name), facts);
        if (member === true) {
          found.set(pattern.name, type);
          return 'fits';
        }
        return member === false ? 'outsideClass' : 'unknown';
      }
      case 'lib': {
        if (
          type.kind !== 'lib' ||
          type.name !== pattern.name ||
          type.args.length !== pattern.args.length
        ) {
          return 'badShape';
        }
        let fit: Fit = 'fits';
        pattern.args.forEach((arg, i) => {
          const argType = type.args[i] ?? ERROR_TYPE;
          const argFit =
            arg.kind === 'type' && !sameType(arg.type, argType)
              ? 'badShape'
              : this.unify(arg, argType, facts, found);
          fit = worse(fit, argFit);
        });
        return fit;
      }
      default:
        return 'fits';
    }
  }
}
