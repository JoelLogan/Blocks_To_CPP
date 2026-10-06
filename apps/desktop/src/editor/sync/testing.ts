/**
 * Test helpers for the editor: the real compiler core, the example and security projects, headless
 * workspaces and loading text into a document. Only tests import this module.
 */
import { type BdmDocument, CoreError, type CoreWasm, initCore } from '@blocks2cpp/b2c-core-wasm';
import { DEFAULT_EDITOR_SERVICES } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { getCore, setCore } from '../../app/core';
import { resetAppStore, useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { type CoreHost, createCoreHost } from '../preview/coreHost';
import type { PreviewService } from '../preview/service';
import {
  createSymbolServices,
  type EditorSymbolServices,
  installEditorServices,
  servicesSessionHooks,
} from '../services';
import { registerEditorBlocks } from '../services/install';
import { withoutEvents } from './bdmToWorkspace';
import { EditorSession } from './session';
import { clearWorkspace } from './traverse';

/** Every example project (examples/*.b2c) by file name. */
export const EXAMPLE_PROJECTS: Readonly<Record<string, string>> = fileMap(
  import.meta.glob<string>('../../../../../examples/*.b2c', {
    query: '?raw',
    import: 'default',
    eager: true,
  }),
);

/** Every malicious project of the security suite (tests/security/projects) by file name. */
export const SECURITY_PROJECTS: Readonly<Record<string, string>> = fileMap(
  import.meta.glob<string>('../../../../../tests/security/projects/*.b2c', {
    query: '?raw',
    import: 'default',
    eager: true,
  }),
);

/** tests/security/projects/README.md: the table of what each malicious project must do. */
const SECURITY_README: string =
  import.meta.glob<string>('../../../../../tests/security/projects/README.md', {
    query: '?raw',
    import: 'default',
    eager: true,
  })['../../../../../tests/security/projects/README.md'] ?? '';

/**
 * The security projects whose Loader column says `accepted` (as the Rust suite reads the table):
 * the ones the round trip must keep byte for byte.
 */
export function acceptedSecurityProjects(): [string, string][] {
  const accepted: [string, string][] = [];
  for (const line of SECURITY_README.split('\n')) {
    if (!line.startsWith('| `')) {
      continue;
    }
    const cells = line.split('|').map((cell) => cell.trim());
    const file = (cells[1] ?? '').replaceAll('`', '');
    const text = SECURITY_PROJECTS[file];
    if (cells[4] === 'accepted' && text !== undefined) {
      accepted.push([file, text]);
    }
  }
  return accepted;
}

function fileMap(modules: Record<string, string>): Record<string, string> {
  const files: Record<string, string> = {};
  for (const [path, text] of Object.entries(modules)) {
    files[path.slice(path.lastIndexOf('/') + 1)] = text;
  }
  return files;
}

/** Whether the tests must have the built compiler core (CI sets `B2C_REQUIRE_WASM`). */
export const WASM_REQUIRED = String(import.meta.env['B2C_REQUIRE_WASM'] ?? '') !== '';

let core: Promise<CoreWasm | null> | undefined;

/**
 * The real compiler core (the built `@blocks2cpp/b2c-core-wasm` module), or `null` when it has not
 * been built (tests that need it are skipped then, unless {@link WASM_REQUIRED}).
 */
export function testCore(): Promise<CoreWasm | null> {
  core ??= initCore().catch((error: unknown) => {
    if (error instanceof CoreError && error.kind === 'init' && !WASM_REQUIRED) {
      console.warn('The compiler core is not built; its tests are skipped', error.message);
      return null;
    }
    throw error;
  });
  return core;
}

/** Loads project text with the core; throws when the loader refuses it. */
export function loadText(wasm: CoreWasm, text: string): BdmDocument {
  const loaded = wasm.load(new TextEncoder().encode(text));
  if (!loaded.ok) {
    throw new Error(
      `The project did not load: ${loaded.diagnostics.map((d) => d.code).join(', ')}`,
    );
  }
  return loaded.document;
}

/** The canonical text of a document; throws when it does not load. */
export function canonicalText(wasm: CoreWasm, doc: BdmDocument): string {
  const canonical = wasm.canonical(JSON.stringify(doc));
  if (!canonical.ok) {
    throw new Error(
      `The document did not load: ${canonical.diagnostics.map((d) => `${d.code} ${d.message}`).join('; ')}`,
    );
  }
  return canonical.text;
}

const workspaces: Blockly.Workspace[] = [];

/** A headless workspace with the editor's blocks registered; dispose it with {@link disposeWorkspaces}. */
export function headlessWorkspace(): Blockly.Workspace {
  registerEditorBlocks();
  const workspace = new Blockly.Workspace();
  workspaces.push(workspace);
  return workspace;
}

/**
 * A rendered workspace (Zelos, with any further `options`) in the document; dispose it with
 * {@link disposeWorkspaces}. Its view has no size (happy-dom lays nothing out, as a hidden editor
 * has none) until {@link setViewSize}.
 */
export function renderedWorkspace(options: Blockly.BlocklyOptions = {}): Blockly.WorkspaceSvg {
  registerEditorBlocks();
  const host = document.createElement('div');
  document.body.append(host);
  const workspace = Blockly.inject(host, { renderer: 'zelos', sounds: false, ...options });
  workspaces.push(workspace);
  return workspace;
}

/**
 * Gives a rendered workspace's container a size, as a browser would lay it out; Blockly reads it at
 * the next `Blockly.svgResize` (the editor calls it through `EditorSession.resize`).
 */
export function setViewSize(workspace: Blockly.WorkspaceSvg, width: number, height: number): void {
  const container = workspace.getParentSvg().parentElement;
  if (container === null) {
    throw new Error('The workspace is not in the document');
  }
  Object.defineProperty(container, 'offsetWidth', { configurable: true, get: () => width });
  Object.defineProperty(container, 'offsetHeight', { configurable: true, get: () => height });
}

/**
 * Disposes the workspaces the helpers made, with Blockly's events off, as the editor does: a delete
 * event serialises the deleted blocks recursively along `next` chains, which overflows the stack
 * for very long chains.
 */
export function disposeWorkspaces(): void {
  for (const workspace of workspaces.splice(0)) {
    clearWorkspace(workspace);
    withoutEvents(() => {
      workspace.dispose();
    });
  }
}

/** `value`, or a failed test naming `what` when it is missing. */
export function present<T>(value: T | null | undefined, what: string): T {
  if (value === null || value === undefined) {
    throw new Error(`${what} is missing`);
  }
  return value;
}

/** A small seeded random number generator (mulberry32), so property tests are reproducible. */
export function seededRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** An editing session on a test workspace, with the editor services installed. */
export interface TestSession {
  readonly workspace: Blockly.Workspace;
  readonly session: EditorSession;
  readonly services: EditorSymbolServices;
  readonly host: CoreHost;
  /** Ends the session and removes the services. */
  dispose(): void;
}

/**
 * Opens `doc` as the project (saved, trusted) and starts a session on `workspace` (a new headless
 * one by default) with `core`. The session loads the document and starts its first preview.
 */
export function startSession(
  core: CoreWasm,
  doc: BdmDocument,
  options: { workspace?: Blockly.Workspace; service?: PreviewService } = {},
): TestSession {
  resetAppStore();
  setCore(core);
  const host = createCoreHost({
    initCore: () => Promise.resolve(core),
    resetCore: () => undefined,
    getCore,
    setCore,
  });
  const text = canonicalText(core, doc);
  useAppStore.getState().actions.setProject(
    projectFixture({
      document: doc,
      canonicalText: text,
      savedCanonicalText: text,
      activeModuleId: doc.modules[0]?.id ?? '',
    }),
  );
  const workspace = options.workspace ?? headlessWorkspace();
  const services = createSymbolServices({
    core: () => host.current(),
    preview: () => useAppStore.getState().analysis.preview,
  });
  const uninstall = installEditorServices({
    symbols: services.symbols,
    types: services.types,
    dialogs: DEFAULT_EDITOR_SERVICES.dialogs,
  });
  const session = new EditorSession({
    workspace,
    store: useAppStore,
    host,
    hooks: servicesSessionHooks(services, workspace),
    ...(options.service === undefined ? {} : { service: options.service }),
  });
  return {
    workspace,
    session,
    services,
    host,
    dispose() {
      session.dispose();
      uninstall();
      setCore(null);
    },
  };
}
