/**
 * The shipped app runs with Tauri's `freezePrototype: true` (docs/spec/08-security.md §8.8). This
 * file freezes `Object.prototype` and then loads and runs the toolbox: `@blockly/continuous-toolbox`,
 * our categories, the dynamic Variables, Loops and Functions categories and *Make a variable*. A
 * library that assigns to an inherited property (`obj.toString = …`) breaks here.
 *
 * Blockly itself is imported before the freeze: under Node its package entry sets up jsdom, whose
 * dependencies assign to inherited properties at load time; the webviews load Blockly's browser
 * build instead, which the end-to-end tests run frozen. Vitest runs each test file in its own
 * isolated environment, so the freeze does not leak into other files.
 */
import { beforeAll, describe, expect, it } from 'vitest';

beforeAll(async () => {
  await import('blockly/core');
  Object.freeze(Object.prototype);
});

describe('with a frozen Object.prototype', () => {
  it('the continuous toolbox shows every category and makes a variable', async () => {
    expect(Object.isFrozen(Object.prototype)).toBe(true);
    const Blockly = await import('blockly/core');
    const testing = await import('./testing');
    const { createToolboxPlugin } = await import('./plugin');
    const { B2cContinuousToolbox } = await import('./continuous');
    const { MAKE_VARIABLE_BUTTON } = await import('./contents');

    testing.setUpToolboxBlocks();
    const workspace = testing.injectedWorkspace('continuous');
    const document = testing.guessDocument();
    testing.loadModule(workspace, document);
    const guess = testing.symbolFixture('s_guess', 'guess', { declBlock: 'decl' });
    const factorial = testing.symbolFixture('s_fact', 'factorial', {
      kind: 'function',
      params: [],
      returns: 'int',
      declBlock: 'fn',
    });
    testing.storeAnalysis(document, [guess, factorial]);
    const created: string[] = [];
    const detach = createToolboxPlugin({
      dialogs: { prompt: () => Promise.resolve('score'), alert: () => Promise.resolve() },
      onMakeVariable: (result) => created.push(result.status),
    }).attach(testing.editorContext(workspace, testing.fakeCore({ print: [factorial, guess] })));
    try {
      const toolbox = workspace.getToolbox();
      expect(toolbox).toBeInstanceOf(B2cContinuousToolbox);
      expect((toolbox as InstanceType<typeof B2cContinuousToolbox>).showAllCategories()).toBe(true);
      const types = (toolbox?.getFlyout()?.getContents() ?? []).flatMap((item) => {
        const element = item.getElement();
        return element instanceof Blockly.BlockSvg ? [element.type] : [];
      });
      expect(types).toContain('var.get');
      expect(types).toContain('func.call');
      expect(types).toContain('control.for_range');

      workspace.getButtonCallback(MAKE_VARIABLE_BUTTON)?.(
        {} as InstanceType<typeof Blockly.FlyoutButton>,
      );
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(created).toEqual(['created']);
    } finally {
      detach();
      testing.disposeTestWorkspaces();
    }
  });
});
