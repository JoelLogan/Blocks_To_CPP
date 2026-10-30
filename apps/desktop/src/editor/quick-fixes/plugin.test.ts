/** The quick-fix plugin: the block menu's items and the fixes the Problems panel asks for. */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { diagnosticFixture, documentFixture } from '../../app/testing/fixtures';
import { createToolboxPlugin } from '../toolbox/plugin';
import {
  FLYOUT_TEST_TIMEOUT_MS,
  disposeTestWorkspaces,
  editorContext,
  fakeCore,
  injectedWorkspace,
  setUpToolboxBlocks,
  storeAnalysis,
} from '../toolbox/testing';
import { QUICK_FIX_MENU_ID, quickFixPlugin, quickFixesFor } from './plugin';

// The toolbox the fixes read its symbols from shows its flyout (FLYOUT_TEST_TIMEOUT_MS).
vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS });

beforeAll(() => {
  setUpToolboxBlocks();
});

beforeEach(() => {
  resetAppStore();
});

const detachers: (() => void)[] = [];

afterEach(() => {
  for (const detach of detachers.splice(0)) {
    detach();
  }
  disposeTestWorkspaces();
  resetAppStore();
});

describe('the quick-fix plugin', () => {
  it('offers a fix to the Problems panel and in the block menu while attached', () => {
    const workspace = injectedWorkspace('continuous');
    const document = documentFixture();
    storeAnalysis(document, []);
    Blockly.Events.disable();
    const stop = Blockly.serialization.blocks.append(
      { type: 'control.break', id: 'stop' },
      workspace,
    );
    Blockly.Events.enable();
    const context = editorContext(workspace, fakeCore({}));
    detachers.push(createToolboxPlugin().attach(context));
    detachers.push(quickFixPlugin.attach(context));
    const noMain = diagnosticFixture({ code: 'B2C-E0405', primary: { part: { kind: 'whole' } } });
    expect(quickFixesFor(noMain).map((fix) => fix.title)).toEqual(['Add “when program starts”']);

    // The block menu offers the fix of a diagnostic on the block.
    const onStop = diagnosticFixture({
      code: 'B2C-E0405',
      primary: { module: 'mod_main', block: 'stop', part: { kind: 'whole' } },
    });
    useAppStore.setState((state) => ({
      analysis: {
        ...state.analysis,
        preview:
          state.analysis.preview === null
            ? null
            : { ...state.analysis.preview, diagnostics: [onStop] },
      },
    }));
    const item = Blockly.ContextMenuRegistry.registry.getItem(`${QUICK_FIX_MENU_ID}_0`) as {
      preconditionFn: (scope: object, event: Event) => string;
      displayText: (scope: object) => string;
    } | null;
    expect(item).not.toBeNull();
    const scope = { block: stop };
    expect(item?.preconditionFn(scope, new PointerEvent('pointerdown'))).toBe('enabled');
    expect(item?.displayText(scope)).toBe('Fix: Add “when program starts”');
    for (const detach of detachers.splice(0)) {
      detach();
    }
    expect(Blockly.ContextMenuRegistry.registry.getItem(`${QUICK_FIX_MENU_ID}_0`)).toBeNull();
    expect(quickFixesFor(noMain)).toEqual([]);
  });
});
