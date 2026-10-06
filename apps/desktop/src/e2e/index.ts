/**
 * The end-to-end test hook (docs/adr/0009-e2e-tooling-and-test-seams.md): `window.__B2C_E2E__`,
 * which the WebDriver tests in apps/desktop/e2e/ call through `executeScript`.
 *
 * It exists only in the frontend built with `vite build --mode e2e` (`pnpm build:e2e`): src/main.tsx
 * imports this module behind `import.meta.env.MODE === 'e2e'`, which every other build replaces
 * with `false`, so the module is not in their bundle at all. CI checks that a release build
 * contains neither this hook's name nor the backend's `B2C_E2E_*` variables.
 *
 * The hook reads the app's state and adds blocks to the open project as the editor itself would;
 * it changes nothing else. Everything it is given comes from the test and is checked.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';

import type { BootPhase } from '../app/bootstrap';
import type { EditorHandle } from '../app/editor-types';
import type { useAppStore } from '../app/store';
import { type ConsoleBridge, documentToBuild } from '../features/build-run';
import type { TrustedTypesReport } from '../lib/trustedTypes';
import { isHiddenFile } from '../panels';
import { insertBlocks } from './blocks';
import { E2E_HOOK_NAME, type E2eHookContract } from './contract';
import {
  blockElement,
  connectionPoint,
  type ElementAtPoint,
  fieldElement,
  flyoutBlockId,
  grabPoint,
} from './locate';
import { ConsoleTranscript, tapConsoleBridge } from './transcript';

export { E2E_HOOK_NAME } from './contract';

/** What the end-to-end tests can call: see ./contract.ts, which the tests import too. */
export type E2eHook = E2eHookContract;

/** What the hook reads. */
export interface E2eHookDeps {
  /** Where the hook is installed (the window). */
  readonly target: object;
  /** The window's start-up phase (`AppRuntime.getPhase`). */
  readonly phase: () => BootPhase;
  readonly store: typeof useAppStore;
  readonly editor: () => EditorHandle | null;
  readonly core: () => CoreWasm | null;
  /** The console bridge, whose output the transcript records. */
  readonly console: ConsoleBridge;
  readonly trustedTypes: () => TrustedTypesReport;
  /** `document.elementFromPoint`, by default. */
  readonly elementAt?: ElementAtPoint;
  /** Parses HTML for {@link E2eHook.probeTrustedTypes}; a `DOMParser` by default. */
  readonly parseHtml?: (html: string) => void;
}

/** The editor's workspace, or an error when there is none. */
function workspaceOf(editor: EditorHandle | null): Blockly.WorkspaceSvg {
  if (editor === null) {
    throw new Error('The editor is not open');
  }
  return editor.workspace;
}

/** A string argument from the test, checked. */
function stringArg(value: unknown, name: string): string {
  if (typeof value !== 'string' || value.length === 0 || value.length > 256) {
    throw new TypeError(`${name} must be a non-empty string of at most 256 characters`);
  }
  return value;
}

/** Field values to match, checked: an object of strings, at most 16 of them. */
function fieldsArg(value: unknown): Readonly<Record<string, string>> {
  if (value === undefined || value === null) {
    return {};
  }
  if (typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError('fields must be an object of field values');
  }
  const entries = Object.entries(value);
  if (entries.length > 16 || !entries.every(([, field]) => typeof field === 'string')) {
    throw new TypeError('fields must hold at most 16 string values');
  }
  return Object.fromEntries(entries);
}

function defaultParseHtml(html: string): void {
  new DOMParser().parseFromString(html, 'text/html');
}

/** Builds the hook over `deps` (without installing it). */
export function createE2eHook(deps: E2eHookDeps, transcript: ConsoleTranscript): E2eHook {
  const elementAt: ElementAtPoint = deps.elementAt ?? ((x, y) => document.elementFromPoint(x, y));
  const parseHtml = deps.parseHtml ?? defaultParseHtml;
  return Object.freeze({
    ready: () => deps.phase().kind === 'ready',
    insertBlocks: (parentBlockId: string, input: string, blocks: readonly unknown[]) => {
      insertBlocks(workspaceOf(deps.editor()), parentBlockId, input, blocks);
    },
    document: () => {
      const found = documentToBuild({ store: deps.store, editor: deps.editor, core: deps.core });
      if (found === null) {
        throw new Error('No project is open');
      }
      if (found.kind === 'unreadable') {
        const codes = found.diagnostics.map((diagnostic) => diagnostic.code).join(', ');
        throw new Error(`The canvas does not read back as a project (${codes})`);
      }
      return found.document.text;
    },
    consoleText: () => transcript.text(),
    trustedTypes: () => {
      const report = deps.trustedTypes();
      return { count: report.count, directives: [...report.directives] };
    },
    selectBlock: (id: string) => {
      const editor = deps.editor();
      if (editor === null) {
        throw new Error('The editor is not open');
      }
      editor.selectBlock(stringArg(id, 'id'), { center: true });
    },
    code: () => {
      const files = deps.store.getState().analysis.preview?.files ?? [];
      return files
        .filter((file) => !isHiddenFile(file.path))
        .map((file) => file.contents)
        .join('\n');
    },
    blockElement: (id: string) => blockElement(workspaceOf(deps.editor()), stringArg(id, 'id')),
    fieldElement: (blockId: string, field: string, input?: string | null) =>
      fieldElement(
        workspaceOf(deps.editor()),
        stringArg(blockId, 'blockId'),
        stringArg(field, 'field'),
        input === undefined || input === null ? undefined : stringArg(input, 'input'),
      ),
    flyoutBlockId: (type: string, fields?: Readonly<Record<string, string>> | null) =>
      flyoutBlockId(workspaceOf(deps.editor()), stringArg(type, 'type'), fieldsArg(fields)),
    connectionPoint: (blockId: string, connection: string) =>
      connectionPoint(
        workspaceOf(deps.editor()),
        stringArg(blockId, 'blockId'),
        stringArg(connection, 'connection'),
      ),
    grabPoint: (blockId: string) =>
      grabPoint(workspaceOf(deps.editor()), stringArg(blockId, 'blockId'), elementAt),
    probeTrustedTypes: () => {
      parseHtml('<p>Blocks2Cpp Trusted Types probe</p>');
    },
  });
}

/**
 * Installs `window.__B2C_E2E__` (read-only) and starts the console transcript. Returns the function
 * that removes both again.
 */
export function installE2eHook(deps: E2eHookDeps): () => void {
  const transcript = new ConsoleTranscript();
  const untap = tapConsoleBridge(deps.console, transcript);
  const hook = createE2eHook(deps, transcript);
  Object.defineProperty(deps.target, E2E_HOOK_NAME, {
    value: hook,
    configurable: true,
    enumerable: false,
    writable: false,
  });
  console.warn('This is an end-to-end test build: window.__B2C_E2E__ is installed');
  return () => {
    Reflect.deleteProperty(deps.target, E2E_HOOK_NAME);
    untap();
  };
}
