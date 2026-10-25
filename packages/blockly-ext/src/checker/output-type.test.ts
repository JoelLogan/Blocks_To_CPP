/**
 * The static output types of reporters and predicates, and the table of every reporter's type
 * against every input type class.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { BLOCK_DEFS, type TypeClass } from '../generated/catalog';
import { registerB2cMutators } from '../mutators/register';
import {
  defineHelperBlocks,
  defineTestBlocks,
  TEST_UNKNOWN_REPORTER,
} from '../mutators/test-fixtures';
import slotFixture from '../types/slot-fixture.json';
import { parseType, toTypeJson, type TypeJson, typeKey } from '../types/static-type';
import { blockDef } from './catalog';
import { conversionAllowed } from './conversion';
import { literalType, staticOutputType } from './output-type';
import type { Compatibility, OutputTypeOracle, StaticType } from './types';

/** M2's seven types by the names its tables use (`string` is `std::string`). */
type M2Name = 'void' | 'bool' | 'char' | 'int' | 'double' | 'string' | 'error';

/** The type object of an M2 name, as the analysis sends it (types.md §3.1). */
function typeNamed(name: M2Name | null): StaticType | null {
  return name === null ? null : parseType(name === 'string' ? 'std::string' : name);
}

/** An answer's M2 name (`string` for `std::string`), or its key for a type of milestone M3. */
function nameOf(type: StaticType | null): string | null {
  if (type === null) {
    return null;
  }
  const key = typeKey(type);
  return key === 'std::string' ? 'string' : key;
}

const CLASSES: readonly TypeClass[] = ['any', 'number', 'integer', 'bool', 'text'];

/** What `conversionAllowed` gives for each type, in the order of `CLASSES` (see conversion.test). */
const ROW: Record<M2Name | 'unknown', readonly Compatibility[]> = {
  unknown: ['ok', 'ok', 'ok', 'ok', 'ok'],
  error: ['ok', 'ok', 'ok', 'ok', 'ok'],
  void: ['invalid', 'invalid', 'invalid', 'invalid', 'invalid'],
  bool: ['ok', 'warning', 'warning', 'ok', 'invalid'],
  char: ['ok', 'ok', 'ok', 'warning', 'ok'],
  int: ['ok', 'ok', 'ok', 'warning', 'invalid'],
  double: ['ok', 'ok', 'warning', 'warning', 'invalid'],
  string: ['ok', 'invalid', 'invalid', 'invalid', 'ok'],
};

const NO_ORACLE: OutputTypeOracle = { outputTypeOf: () => null };

function oracleOf(answer: (block: Blockly.Block) => M2Name | null): OutputTypeOracle {
  return { outputTypeOf: (block) => typeNamed(answer(block)) };
}

let workspace: Blockly.Workspace;

beforeAll(() => {
  registerB2cMutators();
  defineTestBlocks();
  defineHelperBlocks();
});

beforeEach(() => {
  workspace = new Blockly.Workspace();
});

afterEach(() => {
  workspace.dispose();
});

/** The block's static type, by its M2 name. */
function typeOf(block: Blockly.Block, oracle: OutputTypeOracle = NO_ORACLE): string | null {
  const def = blockDef(block.type);
  if (def === undefined) {
    throw new Error(`not a catalog block: ${block.type}`);
  }
  return nameOf(staticOutputType(block, def, oracle));
}

function plug(parent: Blockly.Block, input: string, child: Blockly.Block): void {
  const connection = parent.getInput(input)?.connection;
  if (connection === null || connection === undefined || child.outputConnection === null) {
    throw new Error(`cannot plug into ${input}`);
  }
  connection.connect(child.outputConnection);
}

function number(text: string): Blockly.Block {
  const block = workspace.newBlock('math.number');
  block.setFieldValue(text, 'VALUE');
  return block;
}

function text(): Blockly.Block {
  return workspace.newBlock('text.literal');
}

describe('literalType', () => {
  it('types number literals as b2c-lang does', () => {
    const cases: readonly [string, 'int' | 'double' | 'error'][] = [
      ['0', 'int'],
      ['42', 'int'],
      [' 42 ', 'int'],
      ['-5', 'int'],
      ['+7', 'int'],
      ['007', 'error'],
      ['1.5', 'double'],
      ['.5', 'double'],
      ['1.', 'double'],
      ['1e3', 'double'],
      ['1E-3', 'double'],
      ['2.5e+2', 'double'],
      ["1'000.5", 'double'],
      ['0x1F', 'int'],
      ['0XE', 'int'],
      ['0b101', 'int'],
      ["1'000", 'int'],
      ["0x7F'FF", 'int'],
      ['2147483647', 'int'],
      ['2147483648', 'error'],
      ['-2147483648', 'int'],
      ['-2147483649', 'error'],
      ['0x80000000', 'error'],
      ['', 'error'],
      ['-', 'error'],
      ['abc', 'error'],
      ['1e', 'error'],
      ['1e+', 'error'],
      ['.', 'error'],
      ['1.5.2', 'error'],
      ['1e999', 'error'],
      ["1''0", 'error'],
      ["'1", 'error'],
      ["1'", 'error'],
      ["1.'5", 'error'],
      ["0x'1", 'error'],
      ['0x', 'error'],
      ['0b2', 'error'],
      ['1 000', 'error'],
      ['٣', 'error'],
      ['9'.repeat(401), 'error'],
      [`0x${'F'.repeat(129)}`, 'error'],
    ];
    for (const [literal, expected] of cases) {
      expect(literalType(literal), JSON.stringify(literal)).toBe(expected);
    }
  });
});

describe('staticOutputType', () => {
  it('gives fixed types from the catalog', () => {
    for (const [type, expected] of [
      ['logic.boolean', 'bool'],
      ['logic.not', 'bool'],
      ['logic.operation', 'bool'],
      ['math.compare', 'bool'],
      ['math.random_int', 'int'],
      ['text.literal', 'string'],
      ['text.join', 'string'],
      ['text.char', 'char'],
    ] as const) {
      expect(typeOf(workspace.newBlock(type)), type).toBe(expected);
    }
  });

  it('reads field:TO for math.convert', () => {
    const block = workspace.newBlock('math.convert');
    expect(typeOf(block)).toBe('int');
    block.setFieldValue('double', 'TO');
    expect(typeOf(block)).toBe('double');
  });

  it('types math.number from its literal text', () => {
    expect(typeOf(number('3'))).toBe('int');
    expect(typeOf(number('3.5'))).toBe('double');
    expect(typeOf(number('007'))).toBe('error');
  });

  it('types math.arithmetic from its operands', () => {
    const sum = workspace.newBlock('math.arithmetic');
    // Empty operands are the catalog defaults (0 and 0).
    expect(typeOf(sum)).toBe('int');
    plug(sum, 'A', number('1.5'));
    expect(typeOf(sum)).toBe('double');
    sum.setFieldValue('mod', 'OP');
    expect(typeOf(sum)).toBe('error');

    const remainder = workspace.newBlock('math.arithmetic');
    remainder.setFieldValue('mod', 'OP');
    expect(typeOf(remainder)).toBe('int');

    const withText = workspace.newBlock('math.arithmetic');
    plug(withText, 'B', text());
    expect(typeOf(withText)).toBe('error');

    const withBool = workspace.newBlock('math.arithmetic');
    plug(withBool, 'A', workspace.newBlock('logic.boolean'));
    expect(typeOf(withBool)).toBe('int');

    const nested = workspace.newBlock('math.arithmetic');
    const inner = workspace.newBlock('math.arithmetic');
    plug(inner, 'B', number('2.0'));
    plug(nested, 'A', inner);
    expect(typeOf(nested)).toBe('double');
  });

  it('types + on text with text or a character as text (L16)', () => {
    const typed = (a: M2Name, b: M2Name, op = 'add'): string | null => {
      const sum = workspace.newBlock('math.arithmetic');
      sum.setFieldValue(op, 'OP');
      const left = workspace.newBlock('var.get');
      const right = workspace.newBlock('var.get');
      plug(sum, 'A', left);
      plug(sum, 'B', right);
      return typeOf(
        sum,
        oracleOf((asked) => (asked === left ? a : asked === right ? b : null)),
      );
    };
    expect(typed('string', 'string')).toBe('string');
    expect(typed('string', 'char')).toBe('string');
    expect(typed('char', 'string')).toBe('string');
    // Two characters add their codes, as in C++.
    expect(typed('char', 'char')).toBe('int');
    expect(typed('string', 'int')).toBe('error');
    expect(typed('bool', 'string')).toBe('error');
    expect(typed('string', 'void')).toBe('error');
    expect(typed('string', 'error')).toBe('error');
    expect(typed('string', 'string', 'sub')).toBe('error');
    expect(typed('string', 'char', 'mod')).toBe('error');

    const literals = workspace.newBlock('math.arithmetic');
    plug(literals, 'A', text());
    plug(literals, 'B', text());
    expect(typeOf(literals)).toBe('string');

    // Text with a value the editor does not know yet is not known either.
    const unknown = workspace.newBlock('math.arithmetic');
    plug(unknown, 'A', text());
    plug(unknown, 'B', workspace.newBlock('var.get'));
    expect(typeOf(unknown)).toBeNull();
  });

  it('asks the oracle for operands outside the catalog', () => {
    const sum = workspace.newBlock('math.arithmetic');
    plug(sum, 'A', workspace.newBlock(TEST_UNKNOWN_REPORTER));
    expect(typeOf(sum)).toBeNull();
    expect(
      typeOf(
        sum,
        oracleOf(() => 'double'),
      ),
    ).toBe('double');
    expect(
      typeOf(
        sum,
        oracleOf(() => 'void'),
      ),
    ).toBe('error');
  });

  it('asks the oracle for symbol outputs', () => {
    for (const type of ['var.get', 'func.call'] as const) {
      const block = workspace.newBlock(type);
      expect(typeOf(block)).toBeNull();
      expect(
        typeOf(
          block,
          oracleOf((asked) => (asked === block ? 'string' : null)),
        ),
      ).toBe('string');
    }
  });

  it('treats a failing or confused oracle as not knowing', () => {
    const block = workspace.newBlock('var.get');
    const failing = oracleOf(() => {
      throw new Error('stale analysis');
    });
    expect(typeOf(block, failing)).toBeNull();
    expect(typeOf(block, { outputTypeOf: () => 'list<int>' as unknown as StaticType })).toBeNull();
    expect(typeOf(block, { outputTypeOf: () => 'int' as unknown as StaticType })).toBeNull();
  });

  it('types logic.ternary by the oracle, or else by its two choices', () => {
    const choice = workspace.newBlock('logic.ternary');
    expect(
      typeOf(
        choice,
        oracleOf(() => 'char'),
      ),
    ).toBe('char');
    // Defaults: 0 and 0.
    expect(typeOf(choice)).toBe('int');
    plug(choice, 'THEN', text());
    expect(typeOf(choice)).toBe('error');
    plug(choice, 'ELSE', text());
    expect(typeOf(choice)).toBe('string');

    const mixed = workspace.newBlock('logic.ternary');
    plug(mixed, 'THEN', number('1.5'));
    plug(mixed, 'ELSE', workspace.newBlock('logic.boolean'));
    expect(typeOf(mixed)).toBe('double');

    const unknown = workspace.newBlock('logic.ternary');
    plug(unknown, 'THEN', workspace.newBlock('var.get'));
    expect(typeOf(unknown)).toBeNull();
  });

  it('gives no type for blocks without an output', () => {
    expect(typeOf(workspace.newBlock('io.print'))).toBeNull();
  });

  it('stops following very deep nesting', () => {
    let outer = workspace.newBlock('math.arithmetic');
    const top = outer;
    for (let depth = 0; depth < 200; depth += 1) {
      const inner = workspace.newBlock('math.arithmetic');
      plug(outer, 'A', inner);
      outer = inner;
    }
    expect(typeOf(top)).toBeNull();
  });
});

describe('every reporter against every type class', () => {
  /** The types a reporter can have here: fixed ones, or every oracle answer for the others. */
  function possibleTypes(type: string): (M2Name | null)[] {
    const block = workspace.newBlock(type);
    const def = blockDef(type);
    if (def?.output === 'symbol' || def?.output === 'any') {
      const answers: (M2Name | null)[] = [
        null,
        'void',
        'bool',
        'char',
        'int',
        'double',
        'string',
        'error',
      ];
      return answers.map(
        (answer) =>
          typeOf(
            block,
            oracleOf(() => answer),
          ) as M2Name | null,
      );
    }
    if (type === 'math.convert') {
      return ['int', 'double'].map((to) => {
        block.setFieldValue(to, 'TO');
        return typeOf(block) as M2Name | null;
      });
    }
    if (type === 'math.number') {
      return ['1', '1.5', 'x'].map((literal) => {
        block.setFieldValue(literal, 'VALUE');
        return typeOf(block) as M2Name | null;
      });
    }
    return [typeOf(block) as M2Name | null];
  }

  it('covers every reporter and predicate of the catalog', () => {
    const reporters = BLOCK_DEFS.filter(
      (def) => def.shape === 'reporter' || def.shape === 'predicate',
    );
    expect(reporters.length).toBeGreaterThan(0);
    for (const def of reporters) {
      for (const type of possibleTypes(def.id)) {
        const row = ROW[type ?? 'unknown'];
        CLASSES.forEach((check, column) => {
          expect(
            conversionAllowed(typeNamed(type), check),
            `${def.id} (${String(type)}) into ${check}`,
          ).toBe(row[column]);
        });
      }
    }
  });
});

describe('the M2 output rules of the slot fixture', () => {
  interface OutputCase {
    readonly block: string;
    readonly fields: Readonly<Record<string, string>>;
    readonly inputs: Readonly<Record<string, TypeJson>>;
    readonly type: TypeJson;
  }

  it('type math.number, math.arithmetic and logic.ternary as the analyser does', () => {
    const cases = (slotFixture as unknown as { outputs: readonly OutputCase[] }).outputs;
    expect(cases.length).toBeGreaterThan(100);
    for (const json of cases) {
      const block = workspace.newBlock(json.block);
      for (const [name, value] of Object.entries(json.fields)) {
        block.setFieldValue(value, name);
      }
      const operands = new Map<Blockly.Block, StaticType | null>();
      for (const [name, type] of Object.entries(json.inputs)) {
        const operand = workspace.newBlock('var.get');
        operands.set(operand, parseType(type));
        plug(block, name, operand);
      }
      const def = blockDef(block.type);
      const answer =
        def === undefined
          ? null
          : staticOutputType(block, def, { outputTypeOf: (asked) => operands.get(asked) ?? null });
      expect(
        answer === null ? null : toTypeJson(answer),
        `${json.block} ${JSON.stringify(json.fields)} ${JSON.stringify(json.inputs)}`,
      ).toEqual(json.type);
    }
  });
});
