/**
 * Large documents (02 §2.4.1, 05 §5.4): building and reading are iterative, so a 5,000-block
 * statement list and a 5,000-block loose stack load and read back without a stack overflow, byte
 * for byte.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { afterEach, describe, expect, it } from 'vitest';

import { documentFixture } from '../../app/testing/fixtures';
import { loadModule } from './bdmToWorkspace';
import { useAppStore } from '../../app/store';
import {
  canonicalText,
  disposeWorkspaces,
  headlessWorkspace,
  loadText,
  startSession,
  testCore,
} from './testing';
import { readModule } from './workspaceToBdm';

const core = await testCore();

afterEach(() => {
  disposeWorkspaces();
});

/** `count` print blocks, each with its own text. */
function prints(prefix: string, count: number): BdmBlock[] {
  return Array.from({ length: count }, (_unused, index) => ({
    id: `${prefix}${String(index)}`,
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: `line ${String(index)}` }] } },
  }));
}

/** A document whose main holds `inMain` prints, with a loose stack of `stacked` more. */
function largeDocument(inMain: number, stacked: number): BdmDocument {
  const doc = documentFixture('Large');
  const [head, ...rest] = prints('s', stacked);
  const blocks: BdmBlock[] = [
    {
      id: 'main',
      type: 'program.main',
      v: 1,
      x: 0,
      y: 0,
      statements: { BODY: prints('p', inMain) },
    },
  ];
  if (head !== undefined) {
    blocks.push({ ...head, x: 900, y: 0, ...(rest.length > 0 ? { stack: rest } : {}) });
  }
  doc.modules = [{ id: 'mod_main', name: 'main', workspace: { blocks } }];
  return doc;
}

describe.skipIf(core === null)('large documents', () => {
  it('loads and reads a 5,000-block main and a 5,000-block loose stack', () => {
    if (core === null) {
      return;
    }
    const doc = loadText(core, JSON.stringify(largeDocument(5_000, 5_000)));
    const workspace = headlessWorkspace();
    loadModule(workspace, doc, 'mod_main');
    // Each print has one expression shadow: 10,001 project blocks and 10,000 shadows.
    const all = workspace.getAllBlocks(false);
    expect(all.filter((block) => !block.isShadow()).length).toBe(10_001);
    expect(all.filter((block) => block.isShadow()).length).toBe(10_000);
    const read = readModule(workspace, doc, 'mod_main');
    const stack = read.modules[0]?.workspace.blocks.find((node) => node.id === 's0')?.stack;
    expect(stack?.length).toBe(4_999);
    expect(canonicalText(core, read)).toBe(canonicalText(core, doc));

    // Loading again clears the canvas first, which must not recurse along the chains either.
    loadModule(workspace, read, 'mod_main');
    expect(canonicalText(core, readModule(workspace, read, 'mod_main'))).toBe(
      canonicalText(core, doc),
    );
  }, 120_000);

  it('keeps an editing session working on a 5,000-block document', async () => {
    if (core === null) {
      return;
    }
    const doc = loadText(core, JSON.stringify(largeDocument(5_000, 0)));
    const session = startSession(core, doc);
    try {
      await session.session.flush();
      const { project, analysis } = useAppStore.getState();
      expect(project?.dirty).toBe(false);
      expect(analysis.preview?.files.length).toBeGreaterThan(0);
      expect(canonicalText(core, session.session.currentDocument())).toBe(canonicalText(core, doc));
    } finally {
      session.dispose();
    }
  }, 120_000);
});
