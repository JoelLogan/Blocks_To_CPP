/**
 * The test environment can run the real Blockly: headless workspaces, `Blockly.inject` with the
 * Zelos renderer, and the app's workspace component. Editor tests build on this, so a Blockly or
 * happy-dom update that breaks it fails here first, with a clear name.
 */
import { render } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { EditorWorkspace } from '../editor/EditorWorkspace';

const BLOCK_TYPE = 'b2c_test_environment_say';

beforeAll(() => {
  Blockly.defineBlocksWithJsonArray([
    {
      type: BLOCK_TYPE,
      message0: 'say %1',
      args0: [{ type: 'field_input', name: 'TEXT', text: 'Hello' }],
      previousStatement: null,
      nextStatement: null,
    },
  ]);
});

const workspaces: Blockly.Workspace[] = [];

/** Rendered workspaces that are not part of another one (a flyout or a mutator). */
function mainWorkspaces(): Blockly.Workspace[] {
  return Blockly.Workspace.getAll().filter(
    (workspace) => workspace.rendered && !workspace.isFlyout && !workspace.isMutator,
  );
}

afterEach(() => {
  for (const workspace of workspaces.splice(0)) {
    workspace.dispose();
  }
});

describe('Blockly in the test environment', () => {
  it('runs a headless workspace and round-trips its serialisation', () => {
    const workspace = new Blockly.Workspace();
    workspaces.push(workspace);
    const block = workspace.newBlock(BLOCK_TYPE);
    block.setFieldValue('<b>not markup</b>', 'TEXT');

    const state = Blockly.serialization.workspaces.save(workspace);
    const copy = new Blockly.Workspace();
    workspaces.push(copy);
    Blockly.serialization.workspaces.load(state, copy);

    const [loaded] = copy.getAllBlocks(false);
    expect(loaded?.type).toBe(BLOCK_TYPE);
    expect(loaded?.getFieldValue('TEXT')).toBe('<b>not markup</b>');
  });

  it('injects a rendered Zelos workspace and renders field text as SVG text', () => {
    const host = document.createElement('div');
    document.body.append(host);
    const workspace = Blockly.inject(host, { renderer: 'zelos', sounds: false });
    workspaces.push(workspace);

    const block = workspace.newBlock(BLOCK_TYPE);
    // Blockly shows spaces as no-break spaces, so the probe has none.
    block.setFieldValue('<img/src=x>', 'TEXT');
    block.initSvg();
    block.render();

    expect(host.querySelector('.injectionDiv svg.blocklySvg')).not.toBeNull();
    // User text stays text: it appears in an SVG text node and creates no element.
    const texts = [...block.getSvgRoot().querySelectorAll('text')].map((node) => node.textContent);
    expect(texts.some((text) => text.includes('<img/src=x>'))).toBe(true);
    expect(host.querySelector('img')).toBeNull();
    host.remove();
  });

  it('mounts and disposes the app workspace component', () => {
    const { container, unmount } = render(<EditorWorkspace />);
    expect(container.querySelector('.blockly-host .injectionDiv')).not.toBeNull();
    expect(mainWorkspaces()).toHaveLength(1);

    unmount();
    expect(mainWorkspaces()).toHaveLength(0);
  });
});
