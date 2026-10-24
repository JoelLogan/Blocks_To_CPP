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
import { blockDef } from './catalog';
import { conversionAllowed } from './conversion';
import { literalType, staticOutputType } from './output-type';
import type { Compatibility, OutputTypeOracle, StaticType } from './types';

const CLASSES: readonly TypeClass[] = ['any', 'number', 'integer', 'bool', 'text'];

/** What `conversionAllowed` gives for each type, in the order of `CLASSES` (see conversion.test). */
const ROW: Record<StaticType | 'unknown', readonly Compatibility[]> = {
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

function oracleOf(answer: (block: Blockly.Block) => StaticType | null): OutputTypeOracle {
  return { outputTypeOf: answer };
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

function typeOf(block: Blockly.Block, oracle: OutputTypeOracle = NO_ORACLE): StaticType | null {
  const def = blockDef(block.type);
  if (def === undefined) {
    throw new Error(`not a catalog block: ${block.type}`);
  }
  return staticOutputType(block, def, oracle);
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
    expect(
      typeOf(
        block,
        oracleOf(() => 'list<int>' as StaticType),
      ),
    ).toBeNull();
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
  function possibleTypes(type: string): (StaticType | null)[] {
    const block = workspace.newBlock(type);
    const def = blockDef(type);
    if (def?.output === 'symbol' || def?.output === 'any') {
      const answers: (StaticType | null)[] = [
        null,
        'void',
        'bool',
        'char',
        'int',
        'double',
        'string',
        'error',
      ];
      return answers.map((answer) =>
        typeOf(
          block,
          oracleOf(() => answer),
        ),
      );
    }
    if (type === 'math.convert') {
      return ['int', 'double'].map((to) => {
        block.setFieldValue(to, 'TO');
        return typeOf(block);
      });
    }
    if (type === 'math.number') {
      return ['1', '1.5', 'x'].map((literal) => {
        block.setFieldValue(literal, 'VALUE');
        return typeOf(block);
      });
    }
    return [typeOf(block)];
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
          expect(conversionAllowed(type, check), `${def.id} (${String(type)}) into ${check}`).toBe(
            row[column],
          );
        });
      }
    }
  });
});
