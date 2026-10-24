/**
 * The package's test environment works: the entry point loads, and the package's own Blockly runs
 * both headless and injected with the Zelos renderer. The editor modules' tests build on this.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import * as blocklyExt from '../src/index';

const workspaces: Blockly.Workspace[] = [];

afterEach(() => {
  for (const workspace of workspaces.splice(0)) {
    workspace.dispose();
  }
  document.body.replaceChildren();
});

describe('the blockly-ext test environment', () => {
  it('loads the package entry point with its public API', () => {
    for (const name of [
      'registerB2cBlocks',
      'installIdGenerator',
      'newId',
      'setEditorServices',
      'exprShadowState',
      'readExprShadow',
      'setTokenHighlight',
      'sanitizeFieldText',
      'visibleInvisibles',
      'MUTATOR_FOR_BLOCK',
      'CATEGORY_STYLE',
      'b2cLightTheme',
      'b2cDarkTheme',
    ]) {
      expect(blocklyExt, name).toHaveProperty(name);
    }
  });

  it('runs Blockly headless and injected with Zelos', () => {
    Blockly.defineBlocksWithJsonArray([
      {
        type: 'b2c_test_environment_ext',
        message0: 'value %1',
        args0: [{ type: 'field_number', name: 'N', value: 7 }],
        output: null,
      },
    ]);

    const headless = new Blockly.Workspace();
    workspaces.push(headless);
    expect(headless.newBlock('b2c_test_environment_ext').getFieldValue('N')).toBe(7);

    const host = document.createElement('div');
    document.body.append(host);
    const rendered = Blockly.inject(host, { renderer: 'zelos', sounds: false });
    workspaces.push(rendered);
    const block = rendered.newBlock('b2c_test_environment_ext');
    block.initSvg();
    block.render();
    expect(host.querySelector('.injectionDiv svg.blocklySvg')).not.toBeNull();
  });
});
