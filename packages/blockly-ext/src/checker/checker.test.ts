/**
 * The connection checker: Blockly's own checks kept, the structural rules always, the type rule
 * while dragging, and nothing refused when the program connects blocks (loading, undo).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { registerB2cMutators } from '../mutators/register';
import {
  defineHelperBlocks,
  defineTestBlocks,
  TEST_UNKNOWN_REPORTER,
} from '../mutators/test-fixtures';
import { B2C_CHECKER_NAME, B2cConnectionChecker, registerB2cConnectionChecker } from './checker';
import { setCheckerTypeOracle } from './oracle';
import type { StaticType } from './types';

const FAR = 1e9;

/** Blockly's own checker: what would connect without the b2c rules. */
const PLAIN = new Blockly.ConnectionChecker();

let host: HTMLDivElement;
let workspace: Blockly.WorkspaceSvg;

beforeAll(() => {
  registerB2cMutators();
  defineTestBlocks();
  defineHelperBlocks();
});

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, {
    renderer: 'zelos',
    sounds: false,
    plugins: { connectionChecker: B2C_CHECKER_NAME },
  });
});

afterEach(() => {
  setCheckerTypeOracle(null);
  workspace.dispose();
  document.body.replaceChildren();
  // Restore the catalog test blocks that a test redefined.
  defineTestBlocks();
});

function rendered(type: string): Blockly.BlockSvg {
  const block = workspace.newBlock(type);
  block.initSvg();
  block.render();
  return block;
}

function input(block: Blockly.Block, name: string): Blockly.Connection {
  const connection = block.getInput(name)?.connection;
  if (connection === null || connection === undefined) {
    throw new Error(`no input ${name}`);
  }
  return connection;
}

function output(block: Blockly.Block): Blockly.Connection {
  if (block.outputConnection === null) {
    throw new Error(`${block.type} has no output`);
  }
  return block.outputConnection;
}

function previous(block: Blockly.Block): Blockly.Connection {
  if (block.previousConnection === null) {
    throw new Error(`${block.type} has no previous connection`);
  }
  return block.previousConnection;
}

/** Whether `child` may be dragged into the input `name` of `parent`. */
function canDrop(child: Blockly.Block, parent: Blockly.Block, name: string): boolean {
  return workspace.connectionChecker.canConnect(output(child), input(parent, name), true, FAR);
}

/** Redefines a catalog block type with the wrong connections, as a registration bug would. */
function redefine(type: string, connect: (block: Blockly.Block) => void): void {
  Blockly.Blocks[type] = {
    init(this: Blockly.Block): void {
      this.appendDummyInput().appendField(new Blockly.FieldLabel(type));
      connect(this);
    },
  };
}

describe('registration', () => {
  it('registers B2cConnectionChecker as b2c_checker', () => {
    expect(
      Blockly.registry.getClass(Blockly.registry.Type.CONNECTION_CHECKER, B2C_CHECKER_NAME),
    ).toBe(B2cConnectionChecker);
    expect(workspace.connectionChecker).toBeInstanceOf(B2cConnectionChecker);
  });

  it('can be registered again', () => {
    registerB2cConnectionChecker();
    registerB2cConnectionChecker();
    expect(
      Blockly.registry.getClass(Blockly.registry.Type.CONNECTION_CHECKER, B2C_CHECKER_NAME),
    ).toBe(B2cConnectionChecker);
  });
});

describe('structural rules', () => {
  it('never puts a hat or definition inside anything', () => {
    for (const type of ['program.main', 'func.define']) {
      redefine(type, (block) => {
        block.setPreviousStatement(true);
      });
      const hat = rendered(type);
      const loop = rendered('control.forever');
      const statement = rendered('io.print');
      const checker = workspace.connectionChecker;
      expect(PLAIN.canConnect(previous(hat), input(loop, 'BODY'), false), type).toBe(true);
      expect(checker.canConnect(previous(hat), input(loop, 'BODY'), false), type).toBe(false);
      expect(checker.canConnect(previous(hat), statement.nextConnection, true, FAR), type).toBe(
        false,
      );
    }
  });

  it('never puts a statement into a value input', () => {
    redefine('control.break', (block) => {
      block.setOutput(true);
    });
    const statement = rendered('control.break');
    const print = rendered('io.print');
    expect(PLAIN.canConnect(output(statement), input(print, 'ITEM0'), false)).toBe(true);
    expect(
      workspace.connectionChecker.canConnect(output(statement), input(print, 'ITEM0'), false),
    ).toBe(false);
  });

  it('never puts a reporter or predicate into a statement list', () => {
    for (const type of ['text.literal', 'logic.boolean']) {
      redefine(type, (block) => {
        block.setPreviousStatement(true);
      });
      const reporter = rendered(type);
      const loop = rendered('control.forever');
      expect(PLAIN.canConnect(previous(reporter), input(loop, 'BODY'), false), type).toBe(true);
      expect(
        workspace.connectionChecker.canConnect(previous(reporter), input(loop, 'BODY'), false),
        type,
      ).toBe(false);
    }
  });

  it('lets statements stack and go into statement lists', () => {
    const loop = rendered('control.forever');
    const first = rendered('io.print');
    const second = rendered('var.set');
    const checker = workspace.connectionChecker;
    expect(checker.canConnect(previous(first), input(loop, 'BODY'), true, FAR)).toBe(true);
    expect(checker.canConnect(previous(second), first.nextConnection, true, FAR)).toBe(true);
  });

  it('keeps Blockly’s own safety checks', () => {
    const print = rendered('io.print');
    const checker = workspace.connectionChecker;
    expect(checker.canConnectWithReason(previous(print), print.nextConnection, false)).toBe(
      Blockly.Connection.REASON_SELF_CONNECTION,
    );
    expect(checker.canConnectWithReason(null, print.nextConnection, false)).toBe(
      Blockly.Connection.REASON_TARGET_NULL,
    );
    const literal = rendered('text.literal');
    expect(checker.canConnectWithReason(output(literal), print.nextConnection, false)).toBe(
      Blockly.Connection.REASON_WRONG_TYPE,
    );
  });
});

describe('the type rule while dragging', () => {
  it('refuses text where a number is needed (repeat (…) times)', () => {
    const repeat = rendered('control.repeat');
    expect(
      PLAIN.canConnect(output(rendered('text.literal')), input(repeat, 'TIMES'), true, FAR),
    ).toBe(true);
    expect(canDrop(rendered('text.literal'), repeat, 'TIMES')).toBe(false);
    expect(canDrop(rendered('text.join'), repeat, 'TIMES')).toBe(false);
    expect(
      workspace.connectionChecker.canConnectWithReason(
        output(rendered('text.literal')),
        input(repeat, 'TIMES'),
        true,
        FAR,
      ),
    ).toBe(Blockly.Connection.REASON_CHECKS_FAILED);
  });

  it('allows numbers, narrowing and bool↔number (the analyser only warns)', () => {
    const repeat = rendered('control.repeat');
    const decimal = rendered('math.number');
    decimal.setFieldValue('1.5', 'VALUE');
    expect(canDrop(rendered('math.number'), repeat, 'TIMES')).toBe(true);
    expect(canDrop(decimal, repeat, 'TIMES')).toBe(true);
    expect(canDrop(rendered('logic.boolean'), repeat, 'TIMES')).toBe(true);
    expect(canDrop(rendered('text.char'), repeat, 'TIMES')).toBe(true);
  });

  it('applies each input’s type class', () => {
    const print = rendered('io.print');
    const ask = rendered('io.ask');
    const condition = rendered('control.while');
    expect(canDrop(rendered('text.literal'), print, 'ITEM0')).toBe(true);
    expect(canDrop(rendered('text.char'), ask, 'PROMPT')).toBe(true);
    expect(canDrop(rendered('text.literal'), ask, 'PROMPT')).toBe(true);
    expect(canDrop(rendered('math.random_int'), ask, 'PROMPT')).toBe(false);
    expect(canDrop(rendered('logic.boolean'), ask, 'PROMPT')).toBe(false);
    expect(canDrop(rendered('text.literal'), condition, 'COND')).toBe(false);
    expect(canDrop(rendered('math.compare'), condition, 'COND')).toBe(true);
  });

  it('checks repeated inputs by their base name', () => {
    const operation = rendered('logic.operation');
    expect(canDrop(rendered('text.literal'), operation, 'ITEM1')).toBe(false);
    expect(canDrop(rendered('logic.not'), operation, 'ITEM1')).toBe(true);
  });

  it('asks the oracle for variables and calls, and refuses a void value', () => {
    const repeat = rendered('control.repeat');
    const print = rendered('io.print');
    const types = new Map<Blockly.Block, StaticType>();
    setCheckerTypeOracle({ outputTypeOf: (block) => types.get(block) ?? null });

    const variable = rendered('var.get');
    expect(canDrop(variable, repeat, 'TIMES')).toBe(true);
    types.set(variable, 'string');
    expect(canDrop(variable, repeat, 'TIMES')).toBe(false);
    expect(canDrop(variable, print, 'ITEM0')).toBe(true);

    const call = rendered('func.call');
    types.set(call, 'void');
    expect(canDrop(call, print, 'ITEM0')).toBe(false);
    types.set(call, 'error');
    expect(canDrop(call, repeat, 'TIMES')).toBe(true);
  });

  it('connects types it does not know', () => {
    const repeat = rendered('control.repeat');
    const unknown = rendered(TEST_UNKNOWN_REPORTER);
    expect(canDrop(unknown, repeat, 'TIMES')).toBe(true);
    setCheckerTypeOracle({
      outputTypeOf: () => {
        throw new Error('no analysis yet');
      },
    });
    expect(canDrop(unknown, repeat, 'TIMES')).toBe(true);
    expect(canDrop(rendered('var.get'), repeat, 'TIMES')).toBe(true);
    setCheckerTypeOracle({ outputTypeOf: () => 'string' });
    expect(canDrop(unknown, repeat, 'TIMES')).toBe(false);
  });

  it('does not judge inputs of blocks outside the catalog', () => {
    Blockly.Blocks['test_any_input'] = {
      init(this: Blockly.Block): void {
        this.appendValueInput('X');
        this.setOutput(true);
      },
    };
    const holder = rendered('test_any_input');
    expect(canDrop(rendered('text.literal'), holder, 'X')).toBe(true);
  });
});

describe('connections the program makes', () => {
  it('never refuses a type mismatch, so projects with type errors load', () => {
    const repeat = rendered('control.repeat');
    const literal = rendered('text.literal');
    expect(
      workspace.connectionChecker.canConnect(output(literal), input(repeat, 'TIMES'), false),
    ).toBe(true);
    expect(input(repeat, 'TIMES').connect(output(literal))).toBe(true);
  });

  it('loads a saved type error through Blockly serialization', () => {
    const state: Blockly.serialization.blocks.State = {
      type: 'control.repeat',
      inputs: { TIMES: { block: { type: 'text.literal' } } },
    };
    const block = Blockly.serialization.blocks.append(state, workspace);
    expect(block.getInputTargetBlock('TIMES')?.type).toBe('text.literal');
  });
});

describe('headless workspaces', () => {
  it('apply the structural rules without rendering', () => {
    const headless = new Blockly.Workspace(
      new Blockly.Options({ plugins: { connectionChecker: B2C_CHECKER_NAME } }),
    );
    try {
      redefine('control.break', (block) => {
        block.setOutput(true);
      });
      const statement = headless.newBlock('control.break');
      const print = headless.newBlock('io.print');
      expect(headless.connectionChecker).toBeInstanceOf(B2cConnectionChecker);
      expect(
        headless.connectionChecker.canConnect(output(statement), input(print, 'ITEM0'), false),
      ).toBe(false);
      expect(
        headless.connectionChecker.canConnect(
          output(headless.newBlock('text.literal')),
          input(print, 'ITEM0'),
          false,
        ),
      ).toBe(true);
    } finally {
      headless.dispose();
    }
  });
});
