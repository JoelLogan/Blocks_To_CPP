/**
 * Accessibility of the field editors (docs/spec/04-user-interface.md §4.8): axe-core finds no
 * WCAG 2.2 AA problem in the text editor or the menus the fields open, and user text in them is
 * text, never markup.
 */
import * as Blockly from 'blockly/core';
import { beforeEach, describe, expect, it } from 'vitest';

import { expectNoAxeViolations } from './axe';
import { fakeSymbols, renderedBlock, renderedWorkspace, setUpBlocks, symbol } from './helpers';

beforeEach(() => {
  setUpBlocks();
});

function widgetDiv(): HTMLElement {
  const div = Blockly.WidgetDiv.getDiv();
  if (div === null) {
    throw new Error('no widget div');
  }
  return div;
}

function dropDownContent(): HTMLElement {
  return Blockly.DropDownDiv.getContentDiv();
}

describe('field editors', () => {
  it.each([
    ['math.number', 'VALUE', 'Number'],
    ['text.literal', 'VALUE', 'Text'],
    ['text.char', 'VALUE', 'Character'],
    ['var.declare', 'NAME', 'Name'],
  ])('the %s %s text editor has no axe violations', async (type, name, label) => {
    const block = renderedBlock(renderedWorkspace(), type);
    block.getField(name)?.showEditor();
    const input = widgetDiv().querySelector('input');
    expect(input?.getAttribute('aria-label')).toBe(label);
    await expectNoAxeViolations(widgetDiv());
  });

  it.each([
    ['io.print', 'SEP'],
    ['var.declare', 'TYPE'],
    ['control.while', 'MODE'],
  ])('the %s %s menu has no axe violations', async (type, name) => {
    const block = renderedBlock(renderedWorkspace(), type);
    block.getField(name)?.showEditor();
    expect(
      dropDownContent().querySelector('[role="listbox"]')?.getAttribute('aria-label'),
    ).toBeTruthy();
    await expectNoAxeViolations(dropDownContent());
  });

  it('the symbol menu has no axe violations and shows hostile names as text', async () => {
    const workspace = renderedWorkspace();
    const block = renderedBlock(workspace, 'var.get');
    fakeSymbols(
      { [block.id]: [symbol('s_a', '<b onmouseover=alert(1)>bold</b>'), symbol('s_b', 'beta')] },
      {},
    );
    block.getField('VAR')?.showEditor();
    const menu = dropDownContent();
    expect(menu.querySelector('b')).toBeNull();
    expect(menu.textContent).toContain('<b onmouseover=alert(1)>bold</b>');
    await expectNoAxeViolations(menu);
  });
});
