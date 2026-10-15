/**
 * The preview pipeline with the real compiler core (06 §6.13): each run calls the core once
 * (`update()`), and the unsaved-changes flag follows the canonical text exactly. Moving a block
 * changes only the layout, which the content hash leaves out (05 §5.11), and is still an unsaved
 * change, because a save would write a different file; moving it back leaves nothing unsaved.
 *
 * Without a build of the compiler core these tests are skipped, unless B2C_REQUIRE_WASM is set.
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { afterEach, describe, expect, it } from 'vitest';

import { getCore, setCore } from '../../app/core';
import { resetAppStore, useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { canonicalOf, EXAMPLE_PROJECTS, loadText, present, testCore } from '../sync/testing';
import { createCoreHost } from './coreHost';
import { PreviewPipeline } from './pipeline';

const core = await testCore();

/** `wasm` with every call recorded by method name. */
function recorded(wasm: CoreWasm, calls: string[]): CoreWasm {
  return Object.fromEntries(
    Object.entries(wasm).map(([name, method]) => [
      name,
      (...args: unknown[]) => {
        calls.push(name);
        return (method as (...a: unknown[]) => unknown)(...args);
      },
    ]),
  ) as unknown as CoreWasm;
}

/** `doc` with its first top-level block moved right by `dx`. */
function moved(doc: BdmDocument, dx: number): BdmDocument {
  const copy = structuredClone(doc);
  const top = present(copy.modules[0]?.workspace.blocks[0], 'a top-level block');
  top.x = (top.x ?? 0) + dx;
  return copy;
}

afterEach(() => {
  setCore(null);
  resetAppStore();
});

describe.skipIf(core === null)('the preview pipeline with the real core', () => {
  it('calls update() once per run, and keeps an unsaved move apart from the content hash', async () => {
    const wasm = present(core, 'core');
    const doc = loadText(wasm, present(EXAMPLE_PROJECTS['hello_world.b2c'], 'hello world'));
    const saved = canonicalOf(wasm, doc);
    resetAppStore();
    useAppStore.getState().actions.setProject(
      projectFixture({
        document: doc,
        textHash: saved.textHash,
        contentHash: saved.hash,
        savedTextHash: saved.textHash,
      }),
    );
    const calls: string[] = [];
    const instance = recorded(wasm, calls);
    const host = createCoreHost({
      initCore: () => Promise.resolve(instance),
      resetCore: () => undefined,
      getCore,
      setCore,
    });
    let shown = doc;
    const pipeline = new PreviewPipeline({ store: useAppStore, host, read: () => shown });
    try {
      await pipeline.runNow();
      let project = present(useAppStore.getState().project, 'the project');
      expect(project.dirty).toBe(false);
      expect(project.textHash).toBe(saved.textHash);
      expect(project.contentHash).toBe(saved.hash);
      expect(useAppStore.getState().analysis.preview?.buildable).toBe(true);

      shown = moved(doc, 100);
      await pipeline.runNow();
      project = present(useAppStore.getState().project, 'the project');
      expect(project.contentHash).toBe(saved.hash);
      expect(project.textHash).toBe(canonicalOf(wasm, shown).textHash);
      expect(project.textHash).not.toBe(saved.textHash);
      expect(project.dirty).toBe(true);

      shown = moved(shown, -100);
      await pipeline.runNow();
      expect(useAppStore.getState().project?.dirty).toBe(false);

      expect(calls).toEqual(['update', 'update', 'update']);
    } finally {
      pipeline.dispose();
    }
  });
});
