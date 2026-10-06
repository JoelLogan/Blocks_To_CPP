/**
 * Untrusted clipboard data (docs/spec/05-project-format.md §5.12, 08-security.md): every payload
 * goes through the loader with the limits of a project file, and a refused one changes nothing
 * and is reported with the loader's codes. Also blocks that are fine on their own but would take
 * the project past a limit where they are pasted, including the 32 MiB of its canonical text.
 *
 * Without a build of the core these tests are skipped, unless B2C_REQUIRE_WASM is set (as in CI).
 */
import {
  type BdmBlock,
  type BdmDocument,
  type CoreWasm,
  MAX_DOCUMENT_BYTES,
} from '@blocks2cpp/b2c-core-wasm';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { setEditorHandle } from '../../app/editor-types';
import { GUESSING_GAME_TEXT } from '../diagnostics/testing';
import {
  disposeWorkspaces,
  renderedWorkspace,
  startSession,
  type TestSession,
} from '../sync/testing';
import { anchorForBlock, ON_CANVAS, type PasteAnchor } from './anchor';
import type { ClipboardNotice } from './notices';
import {
  canvasBlock,
  clipboardEditor,
  type ClipboardTestEditor,
  loadGuessingGame,
  nodeOf,
  requireCore,
  sessionController,
  WITH_CORE,
} from './testing';

let core: CoreWasm;
let editor: ClipboardTestEditor | null = null;
let session: TestSession | null = null;

beforeAll(async () => {
  if (WITH_CORE) {
    core = await requireCore();
  }
});

afterEach(() => {
  editor?.dispose();
  editor = null;
  session?.dispose();
  session = null;
  setEditorHandle(null);
  disposeWorkspaces();
});

function open(doc: BdmDocument): ClipboardTestEditor {
  editor = clipboardEditor(core, doc, renderedWorkspace());
  return editor;
}

/** A valid payload: a block of the guessing game (its first print by default), copied by the core. */
function validPayload(id = 'b004'): string {
  const made = core.clipboardMake(JSON.stringify(loadGuessingGame(core)), [id]);
  if (!made.ok) {
    throw new Error('the copy failed');
  }
  return made.payload;
}

/**
 * Pastes `text` after b009 and checks that nothing changed and that the one notice lists exactly
 * the problems the outcome has; returns their codes.
 */
function refusedCodes(
  text: string,
  anchor?: (editor: ClipboardTestEditor) => PasteAnchor,
): string[] {
  const opened = open(loadGuessingGame(core));
  const before = JSON.stringify(opened.document());
  const outcome = opened.clipboard.controller.paste(
    text,
    anchor?.(opened) ?? anchorForBlock(canvasBlock(opened.workspace, 'b009')),
  );
  expect(JSON.stringify(opened.document())).toBe(before);
  expect(outcome.kind).toBe('refused');
  const diagnostics = outcome.kind === 'refused' ? outcome.diagnostics : [];
  expect(opened.notices).toHaveLength(1);
  const notice = opened.notices[0];
  expect(notice?.kind === 'unavailable' ? [] : notice?.diagnostics).toEqual(diagnostics);
  return diagnostics.map((diagnostic) => diagnostic.code);
}

/** `depth` nested `repeat while` loops (from the guessing game's), with a print at the bottom. */
function loops(depth: number, prefix: string): BdmBlock {
  const doc = loadGuessingGame(core);
  const loop = nodeOf(doc, 'b010');
  let node: BdmBlock = { ...nodeOf(doc, 'b004'), id: `${prefix}_leaf` };
  for (let level = depth; level > 0; level -= 1) {
    node = { ...loop, id: `${prefix}${String(level)}`, statements: { BODY: [node] } };
  }
  return node;
}

/** The guessing game with `node` loose on the canvas. */
function withLoose(node: BdmBlock): BdmDocument {
  const doc = loadGuessingGame(core);
  doc.modules[0]?.workspace.blocks.push({ ...node, x: 900, y: 40 });
  return doc;
}

/** Whether the loader takes `doc`. */
function loads(doc: BdmDocument): boolean {
  return core.canonical(JSON.stringify(doc)).ok;
}

describe.skipIf(!WITH_CORE)('malicious payloads', () => {
  it('nesting too deep: B2C-E0104', () => {
    expect(refusedCodes(`${'['.repeat(500)}${']'.repeat(500)}`)).toEqual(['B2C-E0104']);
    const deepBlocks = JSON.stringify({
      format: 'blocks2cpp/clipboard',
      formatVersion: 1,
      catalog: '1.0.0',
      blocks: [loops(80, 'd')],
      refs: {},
    });
    expect(refusedCodes(deepBlocks)).toEqual(['B2C-E0104']);
  });

  it('a key twice in one object: B2C-E0105', () => {
    const twice = validPayload().replace(
      '"catalog": "1.0.0",',
      '"catalog": "1.0.0", "catalog": "1.0.0",',
    );
    expect(refusedCodes(twice)).toEqual(['B2C-E0105']);
  });

  it('prototype keys: B2C-E0127', () => {
    // `ask` refers to `guess`, so its payload has a reference to put another one before.
    const asking = validPayload('b005');
    expect(asking).toContain('"s_guess"');
    const inRefs = asking.replace(
      '"refs": {',
      '"refs": {"__proto__": {"name": "x", "kind": "variable"},',
    );
    expect(refusedCodes(inRefs)).toContain('B2C-E0127');
    const inBlock = validPayload().replace('"id": "b004",', '"id": "b004", "__proto__": {"x": 1},');
    expect(refusedCodes(inBlock)).toContain('B2C-E0127');
    const inFields = validPayload().replace('"fields": {', '"fields": {"constructor": "x",');
    expect(refusedCodes(inFields)).toContain('B2C-E0127');
    // Nothing was polluted on the way.
    expect(({} as Record<string, unknown>)['x']).toBeUndefined();
  });

  it('more than 32 MiB: B2C-E0101', () => {
    const padded = `${' '.repeat(MAX_DOCUMENT_BYTES)}${validPayload()}`;
    expect(refusedCodes(padded)).toEqual(['B2C-E0101']);
  });

  it('another format, or a whole project: B2C-E0138', () => {
    const wrong = validPayload().replace('blocks2cpp/clipboard', 'blockly/clipboard');
    expect(refusedCodes(wrong)).toEqual(['B2C-E0138']);
    expect(refusedCodes(GUESSING_GAME_TEXT)).toEqual(['B2C-E0138']);
  });

  it('a newer format version: B2C-E0108', () => {
    const newer = validPayload().replace('"formatVersion": 1', '"formatVersion": 99');
    expect(refusedCodes(newer)).toEqual(['B2C-E0108']);
  });

  it('not JSON at all: B2C-E0103', () => {
    expect(refusedCodes('')).toEqual(['B2C-E0103']);
    expect(refusedCodes('std::cout << "hi";')).toEqual(['B2C-E0103']);
  });

  it('control and bidirectional characters in names are refused too', () => {
    const doc = loadGuessingGame(core);
    const made = core.clipboardMake(JSON.stringify(doc), ['b003']);
    if (!made.ok) {
      throw new Error('the copy failed');
    }
    const sneaky = made.payload.replace('"name": "guess"', '"name": "gu\\u202eess"');
    expect(refusedCodes(sneaky).length).toBeGreaterThan(0);
  });
});

describe.skipIf(!WITH_CORE)('blocks that fit alone but not where they are pasted', () => {
  it('are refused with the loader’s codes, and still paste where they fit', () => {
    // The deepest nest of loops the document takes loose on the canvas.
    let depth = 1;
    while (loads(withLoose(loops(depth + 1, 'w')))) {
      depth += 1;
    }
    expect(depth).toBeGreaterThan(10);
    const doc = withLoose(loops(depth, 'w'));
    const made = core.clipboardMake(JSON.stringify(doc), ['w1']);
    if (!made.ok) {
      throw new Error('the copy failed');
    }

    // Three lists deep (main, the loop, the if), the copy is too deep.
    const codes = refusedCodes(made.payload, (opened) =>
      anchorForBlock(canvasBlock(opened.workspace, 'b006')),
    );
    expect(codes).toContain('B2C-E0104');
    const notice = editor?.notices[0];
    expect(notice?.kind).toBe('limits');
    editor?.dispose();
    editor = null;
    disposeWorkspaces();

    // On the canvas it fits, as the original does.
    const fits = open(loadGuessingGame(core));
    expect(fits.clipboard.controller.paste(made.payload, ON_CANVAS)).toMatchObject({
      kind: 'pasted',
    });
    expect(fits.notices).toEqual([]);
    expect(loads(fits.document())).toBe(true);
  });
});

describe.skipIf(!WITH_CORE)('a paste whose project file would be larger than 32 MiB', () => {
  /** Blocks whose JSON is far smaller compact than indented, as a project file is written. */
  const COUNT = 70_000;

  it('is refused with B2C-E0101, although the document fits as compact JSON', () => {
    const blocks = Array.from({ length: COUNT }, (_unused, index) => ({
      ...nodeOf(loadGuessingGame(core), 'b004'),
      id: `p${String(index)}`,
      inputs: { ITEM0: { expr: [{ str: 'x' }] } },
    }));
    const text = JSON.stringify({
      format: 'blocks2cpp/clipboard',
      formatVersion: 1,
      catalog: '1.0.0',
      blocks,
      refs: {},
    });
    expect(text.length).toBeLessThan(MAX_DOCUMENT_BYTES / 2);
    // A headless session: should the paste get through, building the blocks takes seconds, not
    // minutes.
    session = startSession(core, loadGuessingGame(core));
    const notices: ClipboardNotice[] = [];
    const controller = sessionController(session, { notices });
    const before = JSON.stringify(session.session.currentDocument());

    const outcome = controller.paste(text, ON_CANVAS);
    const tooLarge = expect.objectContaining({
      code: 'B2C-E0101',
      message: expect.stringContaining('larger than 33554432 bytes (32 MiB)') as unknown,
    }) as unknown;
    // The loader takes the document (no other problem), but its canonical text is too large.
    expect(outcome).toEqual({ kind: 'refused', diagnostics: [tooLarge] });
    expect(notices).toEqual([{ kind: 'limits', action: 'paste', diagnostics: [tooLarge] }]);
    expect(JSON.stringify(session.session.currentDocument())).toBe(before);
  }, 120_000);
});
