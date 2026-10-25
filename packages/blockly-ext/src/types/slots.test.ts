/**
 * The editor's `typeBlock` agrees with `b2c_lang::type_block` on every case of the slot fixture
 * that crates/b2c-lang/tests/fixtures.rs writes (docs/plans/m3-contracts/slots.md §7), field by
 * field; and the catalog blocks' signatures, built from the generated definitions, are the ones
 * the fixture samples.
 */
import { describe, expect, it } from 'vitest';

import { BLOCK_DEFS } from '../generated/catalog';
import {
  blockSignature,
  parsePatternText,
  type SignatureDef,
  type TypePatternJson,
} from './pattern';
import slotFixtureJson from './slot-fixture.json';
import {
  type BlockParts,
  type BlockTyping,
  isFitError,
  type PartType,
  startTokens,
  typeBlock,
} from './slots';
import {
  parseType,
  prim,
  type StaticType,
  type TypeFactsTs,
  type TypeJson,
  toTypeJson,
  type UserTypeTs,
} from './static-type';

interface PartJson {
  readonly kind: string;
  readonly type?: TypeJson;
}

interface PartsJson {
  readonly fields: Readonly<Record<string, PartJson>>;
  readonly inputs: Readonly<Record<string, PartJson>>;
  readonly counts: Readonly<Record<string, number>>;
  readonly params: Readonly<Record<string, readonly PartJson[]>>;
  readonly members: Readonly<Record<string, readonly PartJson[]>>;
  readonly chosenMembers: Readonly<Record<string, PartJson>>;
  readonly result: PartJson | null;
}

interface CaseJson {
  readonly name?: string;
  readonly parts: PartsJson;
  readonly place: TypeJson | null;
  readonly facts: string;
  readonly typing: unknown;
}

/** A definition as Rust serialises it: catalog.json's block shape, or the fixture's own signatures. */
interface RustDefJson {
  readonly output: string | TypePatternJson | null;
  readonly typeVars?: Readonly<Record<string, string>>;
  readonly order?: readonly ({ readonly field: string } | { readonly input: string })[];
  readonly placeVar?: string | null;
  readonly members?: Readonly<Record<string, string>>;
  readonly field: readonly {
    readonly name: string;
    readonly kind: string;
    readonly type?: TypePatternJson | null;
  }[];
  readonly input: readonly {
    readonly name: string;
    readonly check: string;
    readonly type?: TypePatternJson | null;
    readonly assign?: boolean;
    readonly repeat: unknown;
  }[];
}

interface SlotFixture {
  readonly facts: Readonly<Record<string, Readonly<Record<string, UserTypeTs>>>>;
  readonly sigs: Readonly<Record<string, RustDefJson>>;
  readonly cases: readonly (CaseJson & { readonly sig: string; readonly name: string })[];
  readonly catalog: readonly {
    readonly block: string;
    readonly def: RustDefJson;
    readonly cases: readonly CaseJson[];
  }[];
}

const FIXTURE = slotFixtureJson as unknown as SlotFixture;

/** The keys a signature is built from, out of Rust's serde shape. */
function signatureDef(def: RustDefJson): SignatureDef {
  return {
    output: def.output,
    typeVars: def.typeVars ?? {},
    order: def.order ?? [],
    placeVar: def.placeVar ?? null,
    members: def.members ?? {},
    fields: def.field.map((field) => ({
      name: field.name,
      kind: field.kind,
      type: field.type ?? null,
    })),
    inputs: def.input.map((input) => ({
      name: input.name,
      check: input.check,
      type: input.type ?? null,
      assign: input.assign ?? false,
      repeat: input.repeat,
    })),
  };
}

function type(json: TypeJson): StaticType {
  const parsed = parseType(json);
  if (parsed === null) {
    throw new Error(`not a type: ${JSON.stringify(json)}`);
  }
  return parsed;
}

function part(json: PartJson | undefined, otherwise: PartType): PartType {
  if (json === undefined) {
    return otherwise;
  }
  switch (json.kind) {
    case 'empty':
    case 'auto':
    case 'fromPlace':
    case 'unknown':
      return { kind: json.kind };
    case 'known':
      return { kind: 'known', type: type(json.type ?? 'error') };
    default:
      throw new Error(`not a part: ${JSON.stringify(json)}`);
  }
}

const EMPTY: PartType = { kind: 'empty' };
const UNKNOWN: PartType = { kind: 'unknown' };

/** A case's parts, answering as the fixture's table says (fixtures.rs, `Parts`). */
function parts(json: PartsJson): BlockParts {
  const own = <T>(table: Readonly<Record<string, T>>, key: string): T | undefined =>
    Object.hasOwn(table, key) ? table[key] : undefined;
  return {
    field: (name) => part(own(json.fields, name), EMPTY),
    input: (name) => part(own(json.inputs, name), EMPTY),
    count: (base) => own(json.counts, base) ?? 0,
    param: (field, index) => part(own(json.params, field)?.[index], EMPTY),
    member: (field, index) => part(own(json.members, field)?.[index], EMPTY),
    chosenMember: (field) => part(own(json.chosenMembers, field), UNKNOWN),
    result: () => part(json.result ?? undefined, EMPTY),
  };
}

function facts(name: string): TypeFactsTs {
  const table = FIXTURE.facts[name] ?? {};
  return { userType: (id) => (Object.hasOwn(table, id) ? (table[id] ?? null) : null) };
}

/** A typing in the fixture's JSON shape. */
function typingJson(typing: BlockTyping): unknown {
  const sorted = <V, W>(map: ReadonlyMap<string, V>, f: (value: V) => W): Record<string, W> =>
    Object.fromEntries([...map].sort(([a], [b]) => (a < b ? -1 : 1)).map(([k, v]) => [k, f(v)]));
  return {
    bound: sorted(typing.bound, ({ type, binder }) => ({ binder, type: toTypeJson(type) })),
    declares: sorted(typing.declares, toTypeJson),
    fields: sorted(typing.fields, (fit) => fit),
    inputs: sorted(typing.inputs, ({ expects, fit }) => ({
      expects: expects === null ? null : toTypeJson(expects),
      fit,
    })),
    output: typing.output === null ? null : toTypeJson(typing.output),
    place: typing.place,
  };
}

function run(def: SignatureDef, json: CaseJson): unknown {
  const place = json.place === null ? null : type(json.place);
  return typingJson(typeBlock(def, parts(json.parts), place, facts(json.facts)));
}

describe('the slot fixture', () => {
  it('covers the rules it promises', () => {
    expect(FIXTURE.cases.length).toBeGreaterThanOrEqual(25);
    expect(FIXTURE.catalog.map((block) => block.block)).toEqual([
      'func.call',
      'func.call_stmt',
      'func.return',
      'var.declare',
      'var.set',
    ]);
  });

  it('agrees on every hand-written case', () => {
    for (const json of FIXTURE.cases) {
      const sig = FIXTURE.sigs[json.sig];
      expect(sig, json.sig).toBeDefined();
      if (sig === undefined) {
        continue;
      }
      const def = signatureDef(sig);
      expect(blockSignature(def), json.sig).not.toBeNull();
      expect(run(def, json), json.name).toEqual(json.typing);
    }
  });

  it('agrees on every sampled case of the catalog blocks, typed from the generated definitions', () => {
    for (const block of FIXTURE.catalog) {
      const generated = BLOCK_DEFS.find((def) => def.id === block.block);
      expect(generated, block.block).toBeDefined();
      if (generated === undefined) {
        continue;
      }
      // The generated definition gives the signature the fixture sampled.
      const sampled = blockSignature(signatureDef(block.def));
      expect(sampled).not.toBeNull();
      expect(blockSignature(generated), block.block).toEqual(sampled);
      block.cases.forEach((json, i) => {
        expect(run(generated, json), `${block.block} case ${String(i)}`).toEqual(json.typing);
      });
    }
  });
});

describe('signatures of the generated catalog', () => {
  it('read for every block', () => {
    for (const def of BLOCK_DEFS) {
      expect(blockSignature(def), def.id).not.toBeNull();
    }
  });

  it('name the reference and declaration fields and the repeated inputs as build() does', () => {
    const sig = (id: string) => {
      const def = BLOCK_DEFS.find((candidate) => candidate.id === id);
      return def === undefined ? null : blockSignature(def);
    };
    expect(sig('var.declare')?.symbolDecls).toEqual(new Set(['NAME']));
    expect(sig('var.declare')?.symbolRefs).toEqual(new Set());
    expect(sig('var.set')?.symbolRefs).toEqual(new Set(['VAR']));
    expect(sig('func.call')?.repeated).toEqual(new Set(['ARG']));
    expect(sig('func.call_stmt')?.repeated).toEqual(new Set(['ARG']));
    // M2's `symbol` output names the block's one reference field.
    expect(sig('var.get')?.output).toEqual({ kind: 'symbol', field: 'VAR' });
    expect(sig('func.call')?.output).toEqual({ kind: 'symbol', field: 'FUNC' });
    expect(sig('math.convert')?.output).toEqual({ kind: 'field', field: 'TO' });
    expect(sig('text.join')?.output).toEqual({ kind: 'type', type: prim('std::string') });
  });
});

describe('fits and start values', () => {
  it('refuse only conversion and class errors', () => {
    expect(isFitError('invalid')).toBe(true);
    expect(isFitError('outsideClass')).toBe(true);
    for (const fit of ['same', 'widening', 'narrowing', 'boolNumber', 'unknown'] as const) {
      expect(isFitError(fit)).toBe(false);
    }
  });

  it("are M2's for M2's types and none for the others", () => {
    expect(startTokens(prim('int'))).toEqual([{ num: '0' }]);
    expect(startTokens(prim('double'))).toEqual([{ num: '0.0' }]);
    expect(startTokens(prim('bool'))).toEqual([{ kw: 'true' }]);
    expect(startTokens(prim('char'))).toEqual([{ chr: 'a' }]);
    expect(startTokens(prim('std::string'))).toEqual([{ str: '' }]);
    expect(startTokens(prim('void'))).toBeNull();
    expect(startTokens(type({ 'std::vector': 'int' }))).toBeNull();
    expect(startTokens(type({ ref: 'sym_Point' }))).toBeNull();
    expect(startTokens(type('error'))).toBeNull();
  });
});

describe('pattern text', () => {
  it('reads the grammar of types.md §4.2', () => {
    expect(parsePatternText('std::vector<T>')).toEqual({
      kind: 'lib',
      name: 'std::vector',
      args: [{ kind: 'var', name: 'T' }],
    });
    expect(parsePatternText('std::map<int, std::string>')).toEqual({
      kind: 'type',
      type: parseType({ 'std::map': ['int', 'std::string'] }),
    });
    expect(parsePatternText(' elem( C ) ')).toEqual({
      kind: 'fn',
      fn: 'elem',
      of: { kind: 'var', name: 'C' },
    });
    expect(parsePatternText('symbol')).toEqual({ kind: 'symbol', field: null });
    expect(parsePatternText('symbol:MEMBER')).toEqual({ kind: 'symbol', field: 'MEMBER' });
    expect(parsePatternText('param:FUNC')).toEqual({ kind: 'param', field: 'FUNC' });
    expect(parsePatternText('result')).toEqual({ kind: 'result' });
  });

  it('refuses what the catalog refuses', () => {
    for (const text of [
      '',
      'auto',
      'string',
      'float',
      'T3x',
      'std::vector<>',
      'std::vector<int, int>',
      'std::vector<void>',
      'std::map<K>',
      'elem',
      'elem(C',
      'other:X',
      'field:',
      'std::vector<T> x',
      `${'elem('.repeat(9)}C${')'.repeat(9)}`,
      `std::vector<${'x'.repeat(130)}>`,
    ]) {
      expect(parsePatternText(text), text).toBeNull();
    }
    expect(parsePatternText(`${'elem('.repeat(8)}C${')'.repeat(8)}`)).not.toBeNull();
  });
});
