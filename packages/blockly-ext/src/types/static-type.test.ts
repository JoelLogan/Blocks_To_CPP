/**
 * The editor's type rules agree with the analyser's on every case of the shared fixture that
 * crates/b2c-lang/tests/fixtures.rs writes (docs/plans/m3-contracts/types.md §9), and the term
 * reader refuses what the Rust one refuses.
 */
import { describe, expect, it } from 'vitest';

import fixtureJson from './fixture.json';
import { parsePatternJson, parsePatternText, TypeEnv } from './pattern';
import type { BlockParts } from './slots';
import {
  applyTypeFn,
  checkAccepts,
  ERROR_TYPE,
  inClass,
  LIB_TYPES,
  MAX_TYPE_DEPTH,
  parseTerm,
  parseType,
  prim,
  staticAssignment,
  staticConversion,
  type StaticType,
  termFromKey,
  termKey,
  TYPE_CLASSES,
  type TypeClass,
  type TypeFactsTs,
  type TypeFn,
  type TypeJson,
  typeKey,
  typeLabel,
  toTypeJson,
  type UserTypeTs,
} from './static-type';

interface Fixture {
  readonly facts: Readonly<Record<string, Readonly<Record<string, UserTypeTs>>>>;
  readonly libTypes: readonly {
    readonly name: string;
    readonly arity: number;
    readonly label: string;
    readonly header: string;
  }[];
  readonly samples: readonly {
    readonly type: TypeJson;
    readonly key: string | null;
    readonly label: Readonly<Record<string, string>>;
    readonly cpp: Readonly<Record<string, string>>;
  }[];
  readonly conversions: readonly (readonly string[])[];
  readonly assignments: readonly (readonly string[])[];
  readonly classes: Readonly<Record<string, Readonly<Record<string, readonly (boolean | null)[]>>>>;
  readonly checks: Readonly<Record<string, Readonly<Record<string, readonly (boolean | null)[]>>>>;
  readonly typeFns: Readonly<Record<string, readonly (TypeJson | null)[]>>;
  readonly bindings: readonly {
    readonly pattern: unknown;
    readonly vars: Readonly<Record<string, string>>;
    readonly facts: string;
    readonly type: TypeJson;
    readonly fit: string;
    readonly bound: Readonly<Record<string, TypeJson>>;
    readonly resolved: Readonly<Record<string, TypeJson>>;
  }[];
}

const FIXTURE = fixtureJson as unknown as Fixture;

function factsNamed(name: string): TypeFactsTs {
  const table = FIXTURE.facts[name] ?? {};
  return { userType: (id) => (Object.hasOwn(table, id) ? (table[id] ?? null) : null) };
}

const FACT_NAMES = Object.keys(FIXTURE.facts);

function sample(i: number): StaticType {
  const entry = FIXTURE.samples[i];
  const type = entry === undefined ? null : parseType(entry.type);
  if (type === null) {
    throw new Error(`sample ${String(i)} does not read`);
  }
  return type;
}

const SAMPLES: readonly StaticType[] = FIXTURE.samples.map((_, i) => sample(i));

/** Parts that answer nothing (a binding reads none). */
const NO_PARTS: BlockParts = {
  field: () => ({ kind: 'empty' }),
  input: () => ({ kind: 'empty' }),
  count: () => 0,
  param: () => ({ kind: 'empty' }),
  member: () => ({ kind: 'empty' }),
  chosenMember: () => ({ kind: 'unknown' }),
  result: () => ({ kind: 'empty' }),
};

describe('the shared type fixture', () => {
  it('has the samples the Rust side lists', () => {
    expect(SAMPLES.length).toBeGreaterThanOrEqual(28);
    expect(FACT_NAMES).toEqual(['known', 'partial']);
  });

  it('reads, writes and keys every sample as the analyser does', () => {
    FIXTURE.samples.forEach((entry, i) => {
      const type = sample(i);
      expect(toTypeJson(type)).toEqual(entry.type);
      expect(typeKey(type)).toBe(entry.key ?? 'error');
      for (const facts of FACT_NAMES) {
        expect(typeLabel(type, factsNamed(facts), 'friendly'), `label ${facts}`).toBe(
          entry.label[facts],
        );
        expect(typeLabel(type, factsNamed(facts), 'cpp'), `cpp ${facts}`).toBe(entry.cpp[facts]);
      }
    });
  });

  it('converts and assigns every pair of samples as the analyser does', () => {
    SAMPLES.forEach((from, i) => {
      SAMPLES.forEach((to, j) => {
        const where = `${typeKey(from)} to ${typeKey(to)}`;
        expect(staticConversion(from, to), where).toBe(FIXTURE.conversions[i]?.[j]);
        expect(staticAssignment(from, to), where).toBe(FIXTURE.assignments[i]?.[j]);
      });
    });
  });

  it('puts every sample in the classes and checks the analyser does, under both facts', () => {
    for (const facts of FACT_NAMES) {
      const classes = FIXTURE.classes[facts] ?? {};
      const checks = FIXTURE.checks[facts] ?? {};
      expect(Object.keys(classes).sort()).toEqual([...TYPE_CLASSES].sort());
      for (const cls of TYPE_CLASSES) {
        SAMPLES.forEach((type, i) => {
          const where = `${typeKey(type)} in ${cls} (${facts})`;
          expect(inClass(type, cls, factsNamed(facts)), where).toBe(classes[cls]?.[i]);
          expect(checkAccepts(cls, type, factsNamed(facts)), `check ${where}`).toBe(
            checks[cls]?.[i],
          );
        });
      }
    }
  });

  it('applies every type function as the analyser does', () => {
    for (const [fn, row] of Object.entries(FIXTURE.typeFns)) {
      SAMPLES.forEach((type, i) => {
        const answer = applyTypeFn(fn as TypeFn, type);
        expect(answer === null ? null : toTypeJson(answer), `${fn}(${typeKey(type)})`).toEqual(
          row[i],
        );
      });
    }
  });

  it('has the same table of library types', () => {
    expect(LIB_TYPES).toEqual(FIXTURE.libTypes);
  });

  it('binds and resolves every pattern case as the analyser does', () => {
    expect(FIXTURE.bindings.length).toBeGreaterThan(100);
    for (const binding of FIXTURE.bindings) {
      const pattern = parsePatternJson(binding.pattern);
      const type = parseType(binding.type);
      expect(pattern).not.toBeNull();
      expect(type).not.toBeNull();
      if (pattern === null || type === null) {
        continue;
      }
      const vars = new Map(Object.entries(binding.vars) as [string, TypeClass][]);
      const env = new TypeEnv({ vars, placeVar: null, members: new Map() });
      const facts = factsNamed(binding.facts);
      const where = `${JSON.stringify(binding.pattern)} with ${JSON.stringify(binding.type)} (${binding.facts})`;
      expect(env.bind(pattern, { kind: 'known', type }, { field: 'F' }, facts), where).toBe(
        binding.fit,
      );
      const bound: Record<string, TypeJson> = {};
      for (const name of vars.keys()) {
        const entry = env.get(name);
        if (entry !== undefined) {
          bound[name] = toTypeJson(entry.type);
        }
      }
      expect(bound, where).toEqual(binding.bound);
      for (const [text, expected] of Object.entries(binding.resolved)) {
        const derived = parsePatternText(text);
        expect(derived, text).not.toBeNull();
        if (derived === null) {
          continue;
        }
        const r = env.resolve(derived, NO_PARTS, null);
        const answer = r.kind === 'type' ? toTypeJson(r.type) : r.kind;
        expect(answer, `${text} after ${where}`).toEqual(expected);
      }
    }
  });
});

describe('parseType', () => {
  it('reads the wire form and refuses M2 names and auto', () => {
    expect(parseType('std::string')).toEqual(prim('std::string'));
    expect(parseType('error')).toBe(ERROR_TYPE);
    expect(parseType('string')).toBeNull();
    expect(parseType('auto')).toBeNull();
    expect(parseType({ 'std::map': ['std::string', { 'std::vector': 'int' }] })).toEqual({
      kind: 'lib',
      name: 'std::map',
      args: [prim('std::string'), { kind: 'lib', name: 'std::vector', args: [prim('int')] }],
    });
  });

  it('refuses prototype keys and builds fresh objects', () => {
    expect(parseType(JSON.parse('{"__proto__": "int"}'))).toBeNull();
    expect(parseType({ constructor: 'int' })).toBeNull();
    expect(parseType({ prototype: 'int' })).toBeNull();
    expect(parseType({ 'std::vector': JSON.parse('{"__proto__": "int"}') as unknown })).toBeNull();
    const json = { 'std::vector': 'int' };
    const type = parseType(json);
    expect(type).not.toBe(json);
    expect(Object.getPrototypeOf(type)).toBe(Object.prototype);
  });

  it('keeps to 8 levels and 32 names', () => {
    let deep: TypeJson = 'int';
    for (let i = 0; i < MAX_TYPE_DEPTH; i += 1) {
      deep = { 'std::vector': deep };
    }
    expect(parseType(deep)).not.toBeNull();
    expect(parseType({ 'std::vector': deep })).toBeNull();
    // The term reader keeps the limit itself, before any name is looked up.
    expect(parseTerm(deep)).not.toBeNull();
    expect(parseTerm({ 'std::vector': deep })).toBeNull();
    expect(parseTerm({ 'no::such': deep })).toBeNull();
    // A full tree of maps four levels deep has 31 names.
    const tree = (levels: number): TypeJson =>
      levels === 0 ? 'int' : { 'std::map': [tree(levels - 1), tree(levels - 1)] };
    expect(parseTerm({ 'std::vector': tree(4) })).not.toBeNull();
    expect(parseTerm({ 'std::map': [tree(4), 'int'] })).toBeNull();
    // A very deep value is refused without recursing through it.
    let hostile: unknown = 'int';
    for (let i = 0; i < 100_000; i += 1) {
      hostile = { 'std::vector': hostile };
    }
    expect(parseType(hostile)).toBeNull();
  });

  it('refuses unknown names, wrong shapes and arguments that are not values', () => {
    for (const json of [
      'Student',
      'std::vector',
      { 'std::vector': [] },
      { 'std::vector': ['int'] },
      { 'std::vector': ['int', 'int'] },
      { 'std::map': 'int' },
      { 'std::vector': 'void' },
      { 'std::vector': 'std::ifstream' },
      { 'std::vector': 'int', 'std::set': 'int' },
      { ref: 'not an id' },
      { ref: 'sym_x', extra: 1 },
      { vector: 'int' },
      ['int'],
      3,
      null,
      '',
      'int  int',
    ]) {
      expect(parseType(json), JSON.stringify(json)).toBeNull();
    }
    expect(parseType({ ref: 'sym_x' })).toEqual({ kind: 'named', id: 'sym_x' });
  });
});

describe('keys', () => {
  it('are names for names and canonical JSON otherwise, and read back', () => {
    const vector = parseTerm({ 'std::vector': 'int' });
    const map = parseTerm({ 'std::map': ['std::string', { ref: 'sym_x' }] });
    expect(vector === null ? null : termKey(vector)).toBe('{"std::vector":"int"}');
    expect(map === null ? null : termKey(map)).toBe('{"std::map":["std::string",{"ref":"sym_x"}]}');
    expect(termFromKey('{"std::vector":"int"}')).toEqual(vector);
    expect(termFromKey('int')).toEqual({ kind: 'name', name: 'int' });
    // Not canonical, or not a term: not a key.
    expect(termFromKey('{ "std::vector": "int" }')).toBeNull();
    expect(termFromKey('{"std::vector":')).toBeNull();
  });
});
