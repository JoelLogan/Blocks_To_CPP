/** The editor services seam (src/services.ts). */
import { beforeEach, describe, expect, it } from 'vitest';

import {
  DEFAULT_EDITOR_SERVICES,
  getEditorServices,
  resetEditorServices,
  setEditorServices,
} from '../src';
import { nameOf, outputTypeOf, symbolsAt } from '../src/services';
import { headlessWorkspace, setUpBlocks, symbol } from './helpers';

beforeEach(() => {
  setUpBlocks();
});

describe('editor services', () => {
  it('answer safely before the app installs its own', async () => {
    expect(getEditorServices()).toBe(DEFAULT_EDITOR_SERVICES);
    expect(symbolsAt('b1', null)).toEqual([]);
    expect(nameOf('s_x')).toBeNull();
    expect(outputTypeOf(headlessWorkspace().newBlock('math.number'))).toBeNull();
    await expect(DEFAULT_EDITOR_SERVICES.dialogs.prompt('Name?', 'x')).resolves.toBeNull();
    await expect(DEFAULT_EDITOR_SERVICES.dialogs.confirm('Sure?')).resolves.toBe(false);
    await expect(DEFAULT_EDITOR_SERVICES.dialogs.alert('Done')).resolves.toBeUndefined();
  });

  it('use what the app installs, until reset', () => {
    const services = {
      ...DEFAULT_EDITOR_SERVICES,
      symbols: { symbolsAt: () => [symbol('s_a', 'a')], nameOf: () => 'a' },
      types: { outputTypeOf: () => 'int' as const },
    };
    setEditorServices(services);
    expect(getEditorServices()).toBe(services);
    expect(symbolsAt('b1', 'BODY').map((info) => info.id)).toEqual(['s_a']);
    expect(nameOf('s_a')).toBe('a');
    expect(outputTypeOf(headlessWorkspace().newBlock('math.number'))).toBe('int');
    resetEditorServices();
    expect(getEditorServices()).toBe(DEFAULT_EDITOR_SERVICES);
  });

  it('treat a service that throws, or answers nonsense, as one with no answer', () => {
    const fail = () => {
      throw new Error('not ready');
    };
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      symbols: { symbolsAt: fail, nameOf: fail },
      types: { outputTypeOf: fail },
    });
    expect(symbolsAt('b1', null)).toEqual([]);
    expect(nameOf('s_x')).toBeNull();
    expect(outputTypeOf(headlessWorkspace().newBlock('math.number'))).toBeNull();
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      symbols: { symbolsAt: () => 'nonsense' as never, nameOf: () => 42 as never },
    });
    expect(symbolsAt('b1', null)).toEqual([]);
    expect(nameOf('s_x')).toBeNull();
  });
});
