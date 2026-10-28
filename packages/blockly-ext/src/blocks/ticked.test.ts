/**
 * Fields shown only while a checkbox is ticked (./ticked.ts): `when program starts` names its
 * arguments only *with arguments* (docs/plans/m3-contracts/w2.md §1.9).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { installIdGenerator } from '../ids';
import { registerB2cMutators } from '../mutators/register';
import { registerB2cBlocks } from './register';

let workspace: Blockly.WorkspaceSvg;

beforeAll(() => {
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
});

beforeEach(() => {
  const host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, { renderer: 'zelos', sounds: false });
});

afterEach(() => {
  workspace.dispose();
  document.body.replaceChildren();
});

describe('when program starts with arguments', () => {
  it('shows the arguments’ name only while ticked', () => {
    const main = workspace.newBlock('program.main');
    main.initSvg();
    main.render();
    const name = main.getField('ARGV');
    expect(name?.isVisible()).toBe(false);
    main.setFieldValue('TRUE', 'ARGS');
    expect(name?.isVisible()).toBe(true);
    main.setFieldValue('FALSE', 'ARGS');
    expect(name?.isVisible()).toBe(false);
  });

  it('loads ticked with its name shown, and keeps the name while unticked', () => {
    const main = Blockly.serialization.blocks.append(
      {
        type: 'program.main',
        fields: { ARGS: true, ARGV: { sym: 'sym_args', name: 'args' } },
      },
      workspace,
    );
    expect(main.getField('ARGV')?.isVisible()).toBe(true);
    const name = Blockly.serialization.blocks.save(main)?.fields?.['ARGV'] as unknown;
    main.setFieldValue('FALSE', 'ARGS');
    expect(main.getField('ARGV')?.isVisible()).toBe(false);
    expect(Blockly.serialization.blocks.save(main)?.fields?.['ARGV']).toEqual(name);
  });
});
