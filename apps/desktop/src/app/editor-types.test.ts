import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { afterEach, describe, expect, expectTypeOf, it } from 'vitest';

import { getCore, setCore } from './core';
import type { EditorContext, EditorPlugin } from './editor-types';
import type { FeatureContext } from './features';

afterEach(() => {
  setCore(null);
});

describe('EditorContext.core', () => {
  it('is a getter, like FeatureContext.core, that app/core.ts getCore can back', () => {
    expectTypeOf<EditorContext['core']>().toEqualTypeOf<FeatureContext['core']>();
    expectTypeOf(getCore).toEqualTypeOf<EditorContext['core']>();
  });

  it('gives a plugin the core that replaced a trapped one, without reattaching it', () => {
    const first = {} as CoreWasm;
    const second = {} as CoreWasm;
    const seen: (CoreWasm | null)[] = [];
    let ask = (): void => undefined;
    const plugin: EditorPlugin = {
      name: 'reads the core on demand',
      attach: (ctx) => {
        ask = () => {
          seen.push(ctx.core());
        };
        return () => undefined;
      },
    };
    const context = { core: getCore } as Partial<EditorContext> as EditorContext;
    const detach = plugin.attach(context);

    ask();
    setCore(first);
    ask();
    // The preview pipeline recovers from a trap with a new instance (resetCore, initCore, setCore).
    setCore(second);
    ask();
    expect(seen).toEqual([null, first, second]);
    detach();
  });
});
