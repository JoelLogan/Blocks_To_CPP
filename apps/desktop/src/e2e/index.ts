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
import type { CoreVersion, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type { CoreWorkerClient, WorkerLike, WorkerProbe } from '@blocks2cpp/b2c-core-wasm/worker';
import type * as Blockly from 'blockly/core';

import type { BootPhase } from '../app/bootstrap';
import type { EditorHandle } from '../app/editor-types';
import type { useAppStore } from '../app/store';
import { type ConsoleBridge, documentToBuild } from '../features/build-run';
import { latestTimings } from '../lib/timing';
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
  flyoutBlockIds,
  type FlyoutQuery,
  grabPoint,
} from './locate';
import { ConsoleTranscript, tapConsoleBridge } from './transcript';
import { startWorkerCheck, type WorkerCheck } from './workerCheck';

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
  /** How many of those violations were Trusted Types' own (`require-trusted-types-for`). */
  readonly trustedTypesCount: () => number;
  /** `document.elementFromPoint`, by default. */
  readonly elementAt?: ElementAtPoint;
  /** Parses HTML for {@link E2eHook.probeTrustedTypes}; a `DOMParser` by default. */
  readonly parseHtml?: (html: string) => void;
  /** Creates a preview worker client for {@link E2eHook.startWorkerCheck}. */
  readonly workerClient: (acceptVersion: (version: CoreVersion) => boolean) => CoreWorkerClient;
  /** Creates the preview worker from a plain string URL, without the Trusted Types policy. */
  readonly spawnUnwrappedWorker: () => WorkerLike;
  /** The worker probe's request to the IPC endpoint, made from the page. */
  readonly pageRequest: () => Promise<WorkerProbe['request']>;
  /** Whether this build previews in the worker (unless built with `B2C_PREVIEW_WORKER=0`). */
  readonly previewWorkerEnabled: () => boolean;
  /** The open editor's worker preview service, or `null`. */
  readonly previewService: () => {
    thread(): 'worker' | 'main';
    client(): CoreWorkerClient | null;
  } | null;
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

/** The farthest {@link E2eHook.scrollCanvasBy} scrolls in one call, in CSS pixels. */
const MAX_SCROLL = 1_000_000;

/** A scroll distance from the test, checked: a finite number of at most {@link MAX_SCROLL}. */
function scrollArg(value: unknown, name: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || Math.abs(value) > MAX_SCROLL) {
    throw new TypeError(
      `${name} must be a number of pixels from -${String(MAX_SCROLL)} to ${String(MAX_SCROLL)}`,
    );
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

/** The most queries {@link E2eHook.flyoutBlockIds} takes in one call. */
const MAX_FLYOUT_QUERIES = 64;

/** Flyout queries from the test, checked: at most {@link MAX_FLYOUT_QUERIES} of them. */
function flyoutQueriesArg(value: unknown): FlyoutQuery[] {
  if (!Array.isArray(value) || value.length > MAX_FLYOUT_QUERIES) {
    throw new TypeError(
      `queries must be an array of at most ${String(MAX_FLYOUT_QUERIES)} flyout queries`,
    );
  }
  return value.map((query: unknown) => {
    if (typeof query !== 'object' || query === null || Array.isArray(query)) {
      throw new TypeError('each query must be an object with a type and fields');
    }
    const { type, fields } = query as { type?: unknown; fields?: unknown };
    return { type: stringArg(type, 'type'), fields: fieldsArg(fields) };
  });
}

function defaultParseHtml(html: string): void {
  new DOMParser().parseFromString(html, 'text/html');
}

/** Builds the hook over `deps` (without installing it). */
export function createE2eHook(deps: E2eHookDeps, transcript: ConsoleTranscript): E2eHook {
  const elementAt: ElementAtPoint = deps.elementAt ?? ((x, y) => document.elementFromPoint(x, y));
  const parseHtml = deps.parseHtml ?? defaultParseHtml;
  let check: WorkerCheck | null = null;
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
    consoleScreen: () => {
      const screen = deps.console.console().screen();
      return { ...screen, lines: [...screen.lines], cursor: { ...screen.cursor } };
    },
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
    scrollCanvasBy: (dx: number, dy: number) => {
      const x = scrollArg(dx, 'dx');
      const y = scrollArg(dy, 'dy');
      const workspace = workspaceOf(deps.editor());
      // What a drag of the background does (Blockly's WorkspaceDragger), without the gesture.
      workspace.scroll(workspace.scrollX + x, workspace.scrollY + y);
    },
    code: () => {
      const files = deps.store.getState().analysis.preview?.files ?? [];
      return files
        .filter((file) => !isHiddenFile(file.path))
        .map((file) => file.contents)
        .join('\n');
    },
    timings: () => latestTimings(),
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
    flyoutBlockIds: (queries: readonly unknown[]) =>
      flyoutBlockIds(workspaceOf(deps.editor()), flyoutQueriesArg(queries)),
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
    startWorkerCheck: (rounds: number) => {
      check = startWorkerCheck(
        {
          documentJson: () => {
            const doc = deps.store.getState().project?.document;
            return doc === undefined ? null : JSON.stringify(doc);
          },
          core: deps.core,
          options: () => ({
            indentWidth: deps.store.getState().settings.value?.codeStyle.indentWidth ?? 4,
          }),
          client: deps.workerClient,
          spawnUnwrapped: deps.spawnUnwrappedWorker,
          trustedTypesViolations: deps.trustedTypesCount,
          pageRequest: deps.pageRequest,
        },
        rounds,
      );
    },
    workerCheck: () => check?.result() ?? null,
    previewThread: () => {
      if (!deps.previewWorkerEnabled()) {
        return 'off';
      }
      return deps.previewService()?.thread() ?? null;
    },
    stopPreviewWorker: () => {
      const client = deps.previewService()?.client() ?? null;
      if (client === null) {
        throw new Error('The editor has no preview worker');
      }
      client.kill();
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
