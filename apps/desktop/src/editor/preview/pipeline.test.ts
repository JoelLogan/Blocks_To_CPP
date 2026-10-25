/**
 * The live preview pipeline with stand-in cores (docs/spec/06-compiler-pipeline.md §6.13): the
 * debounce, one `update()` per run, discarding results that newer runs overtook, the dirty state
 * from the hash of the canonical text, trap recovery, keeping an edit the preview could not
 * handle, and the notice when the editor's document does not load.
 */
import {
  type BdmDocument,
  type CanonicalResult,
  CoreError,
  CoreTrap,
  type CoreWasm,
  type Diagnostic,
  type PreviewOptions,
  type UpdateResult,
} from '@blocks2cpp/b2c-core-wasm';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import {
  diagnosticFixture,
  documentFixture,
  previewFixture,
  projectFixture,
  settingsFixture,
} from '../../app/testing/fixtures';
import { latestTimings, resetTimings } from '../../lib/timing';
import { type CoreHost, createCoreHost } from './coreHost';
import { PreviewPipeline } from './pipeline';
import type { PreviewService } from './service';

/** The content hash the stand-in cores report. */
const CONTENT_HASH = 'b'.repeat(64);

/**
 * What the stand-in cores report as the hash of a document's canonical text: the JSON text
 * itself, which is equal for two documents exactly when the real hash is.
 */
function textHashOf(json: string): string {
  return `hash of ${json}`;
}

/** An update of a document that loaded, with `diagnostics`. */
function updateFixture(json: string, diagnostics: Diagnostic[] = []): UpdateResult {
  return { ...previewFixture(diagnostics), contentHash: CONTENT_HASH, textHash: textHashOf(json) };
}

/** An update of a document that did not load, with the loader's `diagnostics`. */
function notLoaded(diagnostics: Diagnostic[]): UpdateResult {
  return {
    stage: 'load',
    diagnostics,
    files: [],
    sourceMap: null,
    buildable: false,
    placeholders: 0,
    contentHash: null,
    blockTypes: {},
    slotTypes: {},
    symbols: [],
    textHash: null,
  };
}

/** A stand-in core: `update` previews a fixture, `canonical` writes the JSON text itself. */
function stubCore(overrides: Partial<CoreWasm> = {}): CoreWasm {
  const unused = (): never => {
    throw new Error('not used by these tests');
  };
  return {
    version: unused,
    load: unused,
    canonical: (json: string): CanonicalResult => ({
      ok: true,
      text: json,
      hash: CONTENT_HASH,
      textHash: textHashOf(json),
      diagnostics: [],
    }),
    update: (json: string): UpdateResult => updateFixture(json),
    symbolsInScope: () => [],
    conversionTable: unused,
    clipboardMake: unused,
    pastePrepare: unused,
    ...overrides,
  };
}

/** A core whose instance has stopped: every call traps, as a real one does after a trap. */
function trappedCore(): CoreWasm {
  const trap = (): never => {
    throw new CoreTrap('unreachable');
  };
  return stubCore({ update: trap, canonical: trap });
}

/** A host over a list of cores: each `restart` moves to the next one. */
function hostOf(...cores: CoreWasm[]): CoreHost & { restarts: number } {
  let index = 0;
  let current: CoreWasm | null = null;
  const host = createCoreHost({
    initCore: () => {
      const core = cores[Math.min(index, cores.length - 1)];
      return core === undefined
        ? Promise.reject(new CoreError('init', 'no core'))
        : Promise.resolve(core);
    },
    resetCore: () => {
      index += 1;
      result.restarts += 1;
    },
    getCore: () => current,
    setCore: (core) => {
      current = core;
    },
  });
  const result = Object.assign(host, { restarts: 0 });
  return result;
}

/** A preview service whose answers the test releases by hand, in any order. */
function manualService(): PreviewService & {
  calls: { json: string; options: PreviewOptions; resolve: (result: UpdateResult) => void }[];
} {
  const calls: {
    json: string;
    options: PreviewOptions;
    resolve: (result: UpdateResult) => void;
  }[] = [];
  return {
    calls,
    update: (json, options) =>
      new Promise((resolve) => {
        calls.push({ json, options, resolve });
      }),
  };
}

let docVersion = 0;
/** The document the pipeline reads: a fixture whose name changes with every edit. */
function readDocument(): BdmDocument {
  return documentFixture(`Edit ${String(docVersion)}`);
}

/** The project, saved as the document of edit 0, but holding an older text. */
function openProject(): void {
  useAppStore.getState().actions.setProject(
    projectFixture({
      textHash: 'older',
      savedTextHash: textHashOf(JSON.stringify(readDocument())),
    }),
  );
}

beforeEach(() => {
  resetAppStore();
  docVersion = 0;
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('the preview pipeline', () => {
  it('waits for changes to settle for 50 ms, then runs once', async () => {
    openProject();
    const stub = stubCore();
    const update = vi.fn((json: string, options: PreviewOptions) => stub.update(json, options));
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore({ update })),
      read: readDocument,
    });
    pipeline.schedule();
    await vi.advanceTimersByTimeAsync(30);
    pipeline.schedule();
    await vi.advanceTimersByTimeAsync(30);
    pipeline.schedule();
    expect(pipeline.pending).toBe(true);
    await vi.advanceTimersByTimeAsync(49);
    expect(update).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(update).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().analysis.preview).not.toBeNull();
    pipeline.dispose();
  });

  it('calls the core once per run: update(), never canonical() or anything else', async () => {
    openProject();
    const calls: string[] = [];
    const stub = stubCore();
    const spied = Object.fromEntries(
      Object.entries(stub).map(([name, method]) => [
        name,
        (...args: unknown[]) => {
          calls.push(name);
          return (method as (...a: unknown[]) => unknown)(...args);
        },
      ]),
    ) as unknown as CoreWasm;
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(spied),
      read: readDocument,
    });
    await pipeline.runNow();
    docVersion = 1;
    await pipeline.runNow();
    expect(calls).toEqual(['update', 'update']);
    expect(useAppStore.getState().project?.document.project.name).toBe('Edit 1');
    pipeline.dispose();
  });

  it('makes the read document the project, with its hashes, dirty only when its text hash differs from the saved one', async () => {
    openProject();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
    });
    await pipeline.runNow();
    let project = useAppStore.getState().project;
    expect(project?.document.project.name).toBe('Edit 0');
    expect(project?.textHash).toBe(textHashOf(JSON.stringify(readDocument())));
    expect(project?.contentHash).toBe(CONTENT_HASH);
    expect(project?.dirty).toBe(false);

    docVersion = 1;
    await pipeline.runNow();
    project = useAppStore.getState().project;
    expect(project?.document.project.name).toBe('Edit 1');
    // The content hash is the same here (the stand-in reports one), the text hash is not.
    expect(project?.contentHash).toBe(CONTENT_HASH);
    expect(project?.dirty).toBe(true);

    docVersion = 0;
    await pipeline.runNow();
    expect(useAppStore.getState().project?.dirty).toBe(false);
    pipeline.dispose();
  });

  it('discards a preview that a newer run overtook', async () => {
    openProject();
    const service = manualService();
    const onPreviewed = vi.fn();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
      service,
      onPreviewed,
    });
    const first = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    docVersion = 1;
    const second = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    expect(service.calls).toHaveLength(2);

    const newer = updateFixture(service.calls[1]?.json ?? '', [
      diagnosticFixture({ code: 'B2C-E0203' }),
    ]);
    service.calls[1]?.resolve(newer);
    await second;
    service.calls[0]?.resolve(
      updateFixture(service.calls[0].json, [diagnosticFixture({ code: 'B2C-E0201' })]),
    );
    await first;

    expect(useAppStore.getState().analysis.preview).toBe(newer);
    expect(useAppStore.getState().analysis.seq).toBe(2);
    expect(useAppStore.getState().project?.document.project.name).toBe('Edit 1');
    expect(onPreviewed).toHaveBeenCalledTimes(1);
    pipeline.dispose();
  });

  it('drops results for a project that was closed or replaced meanwhile', async () => {
    openProject();
    const service = manualService();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
      service,
    });
    const run = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    useAppStore
      .getState()
      .actions.setProject(projectFixture({ handle: 'ph_ffffffffffffffffffffffffffffffff' }));
    service.calls[0]?.resolve(updateFixture(service.calls[0].json, [diagnosticFixture()]));
    await run;
    expect(useAppStore.getState().analysis.preview).toBeNull();
    expect(useAppStore.getState().project?.document.project.name).toBe('Guessing Game');
    pipeline.dispose();
  });

  it('previews with the indent width of the settings, and again when it changes', async () => {
    openProject();
    useAppStore.getState().actions.setSettings({ value: settingsFixture() });
    const service = manualService();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
      service,
    });
    void pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    expect(service.calls[0]?.options).toEqual({ indentWidth: 4 });

    useAppStore
      .getState()
      .actions.setSettings({ value: settingsFixture({ codeStyle: { indentWidth: 2 } }) });
    await vi.advanceTimersByTimeAsync(50);
    expect(service.calls[1]?.options).toEqual({ indentWidth: 2 });
    pipeline.dispose();
  });

  it('replaces a trapped core, runs again on the new one, and says so', async () => {
    openProject();
    const healthy = stubCore();
    const host = hostOf(trappedCore(), healthy);
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const pipeline = new PreviewPipeline({ store: useAppStore, host, read: readDocument });
    await pipeline.runNow();

    expect(host.restarts).toBe(1);
    expect(host.current()).toBe(healthy);
    const { analysis, project } = useAppStore.getState();
    expect(analysis.preview).not.toBeNull();
    expect(analysis.notice).toBe('trapRecovered');
    expect(project?.textHash).toBe(textHashOf(JSON.stringify(readDocument())));
    expect(error).toHaveBeenCalled();
    pipeline.dispose();
  });

  it('keeps the last good preview, but still the edit, when the new core traps as well', async () => {
    openProject();
    const good = previewFixture([diagnosticFixture()]);
    useAppStore.getState().actions.setAnalysis({ preview: good });
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    // The update traps on the first and the second instance; the third one writes the document.
    const host = hostOf(trappedCore(), trappedCore(), stubCore());
    const pipeline = new PreviewPipeline({ store: useAppStore, host, read: readDocument });
    docVersion = 1;
    await pipeline.runNow();
    const { analysis, project } = useAppStore.getState();
    expect(analysis.preview).toBe(good);
    expect(analysis.notice).toBe('trapRecovered');
    expect(host.restarts).toBe(2);
    // The edit is the project's, and counts as unsaved (autosave keeps it, closing asks).
    expect(project?.document.project.name).toBe('Edit 1');
    expect(project?.textHash).toBe(textHashOf(JSON.stringify(readDocument())));
    expect(project?.dirty).toBe(true);
    pipeline.dispose();
  });

  it('keeps the project as it was when not even canonical() works on a new core', async () => {
    openProject();
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const host = hostOf(trappedCore(), trappedCore(), trappedCore());
    const pipeline = new PreviewPipeline({ store: useAppStore, host, read: readDocument });
    docVersion = 1;
    await pipeline.runNow();
    const { analysis, project } = useAppStore.getState();
    expect(analysis.notice).toBe('trapRecovered');
    expect(project?.document.project.name).toBe('Guessing Game');
    expect(project?.textHash).toBe('older');
    pipeline.dispose();
  });

  it('keeps the last good preview when the document does not load (a sync bug)', async () => {
    openProject();
    const good = previewFixture();
    useAppStore.getState().actions.setAnalysis({ preview: good });
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(
        stubCore({ update: () => notLoaded([diagnosticFixture({ code: 'B2C-E0114' })]) }),
      ),
      read: readDocument,
    });
    await pipeline.runNow();
    const { analysis, project } = useAppStore.getState();
    expect(analysis.preview).toBe(good);
    expect(analysis.notice).toBe('syncFailed');
    // With the loader's codes, for the banner.
    expect(analysis.syncCodes).toEqual(['B2C-E0114']);
    expect(project?.textHash).toBe('older');
    // The log names the codes only, never project text.
    expect(String(error.mock.calls[0]?.[0])).toContain('B2C-E0114');
    pipeline.dispose();
  });

  it('clears the sync notice after a run that works', async () => {
    openProject();
    useAppStore.getState().actions.setAnalysis({ notice: 'syncFailed' });
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
    });
    await pipeline.runNow();
    expect(useAppStore.getState().analysis.notice).toBeNull();
    pipeline.dispose();
  });

  it('keeps the edit and clears the sync notice when the canvas loads again, even if the preview fails', async () => {
    openProject();
    useAppStore.getState().actions.setAnalysis({ notice: 'syncFailed' });
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
      service: { update: () => Promise.reject(new Error('the preview failed')) },
    });
    docVersion = 1;
    await pipeline.runNow();
    const { analysis, project } = useAppStore.getState();
    expect(analysis.notice).toBeNull();
    expect(analysis.preview).toBeNull();
    expect(project?.document.project.name).toBe('Edit 1');
    expect(project?.dirty).toBe(true);
    expect(error).toHaveBeenCalledWith('The live preview failed', expect.any(Error));
    pipeline.dispose();
  });

  it('notifies the store’s subscribers at most twice per run', async () => {
    openProject();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
    });
    let notifications = 0;
    const unsubscribe = useAppStore.subscribe(() => {
      notifications += 1;
    });
    try {
      docVersion = 1;
      await pipeline.runNow();
      expect(notifications).toBe(2);
      expect(useAppStore.getState().analysis.seq).toBe(1);
      expect(useAppStore.getState().project?.document.project.name).toBe('Edit 1');

      // Also when the canvas loads again after a sync failure, which clears the notice.
      useAppStore.getState().actions.setAnalysis({ notice: 'syncFailed' });
      notifications = 0;
      docVersion = 2;
      await pipeline.runNow();
      expect(notifications).toBe(2);
      expect(useAppStore.getState().analysis).toMatchObject({ seq: 2, notice: null });
    } finally {
      unsubscribe();
      pipeline.dispose();
    }
  });

  it('measures each run and its phases as User Timing spans', async () => {
    resetTimings();
    openProject();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
    });
    await pipeline.runNow();
    const timings = latestTimings();
    for (const span of [
      'b2c:preview',
      'b2c:preview:read',
      'b2c:preview:update',
      'b2c:preview:commit',
      'b2c:preview:publish',
    ] as const) {
      expect(timings[span], span).toBeGreaterThanOrEqual(0);
    }
    pipeline.dispose();

    // A run whose document does not load is not a preview: nothing more is recorded.
    resetTimings();
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const failing = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore({ update: () => notLoaded([]) })),
      read: readDocument,
    });
    await failing.runNow();
    expect(latestTimings()['b2c:preview']).toBeNull();
    expect(latestTimings()['b2c:preview:update']).not.toBeNull();
    expect(latestTimings()['b2c:preview:commit']).toBeNull();
    failing.dispose();
  });

  it('does nothing without a project, or when the core cannot start', async () => {
    const read = vi.fn(readDocument);
    const pipeline = new PreviewPipeline({ store: useAppStore, host: hostOf(), read });
    await pipeline.runNow();
    expect(read).not.toHaveBeenCalled();

    openProject();
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await pipeline.runNow();
    expect(read).not.toHaveBeenCalled();
    expect(error).toHaveBeenCalledWith(
      'The compiler core could not be started',
      expect.any(CoreError),
    );
    pipeline.dispose();
  });

  it('is idle once a run has committed what it read, until another read is asked for', async () => {
    openProject();
    const service = manualService();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
      service,
    });
    const answer = (index: number, result?: UpdateResult): void => {
      const call = service.calls[index];
      if (call === undefined) {
        throw new Error(`no update call ${String(index)}`);
      }
      call.resolve(result ?? updateFixture(call.json));
    };
    // Nothing read yet.
    expect(pipeline.isIdle()).toBe(false);

    // A run that has started but not read the canvas yet (the core starts first).
    const first = pipeline.runNow();
    expect(pipeline.isIdle()).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    // Read, but the update has not answered: the project does not hold the read document yet.
    expect(service.calls).toHaveLength(1);
    expect(pipeline.isIdle()).toBe(false);
    answer(0);
    await first;
    expect(pipeline.isIdle()).toBe(true);

    // An edit waits for its run, then for its update.
    pipeline.schedule();
    expect(pipeline.isIdle()).toBe(false);
    await vi.advanceTimersByTimeAsync(50);
    expect(service.calls).toHaveLength(2);
    expect(pipeline.isIdle()).toBe(false);
    answer(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(pipeline.isIdle()).toBe(true);

    // A new document is shown: what was committed is no longer the canvas.
    pipeline.invalidate();
    expect(pipeline.isIdle()).toBe(false);
    const second = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    answer(2);
    await second;
    expect(pipeline.isIdle()).toBe(true);

    // A canvas that does not load keeps the older document: not idle.
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const failing = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    answer(3, notLoaded([diagnosticFixture({ code: 'B2C-E0104' })]));
    await failing;
    expect(useAppStore.getState().analysis.notice).toBe('syncFailed');
    expect(pipeline.isIdle()).toBe(false);
    const third = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    answer(4);
    await third;
    expect(pipeline.isIdle()).toBe(true);
    pipeline.dispose();
  });

  it('is not idle while the canvas cannot be read and the read is put off', async () => {
    openProject();
    let reads = 0;
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      // What the session does during a drag: no document now, a run later.
      read: () => {
        reads += 1;
        if (reads === 2) {
          pipeline.schedule();
          return null;
        }
        return readDocument();
      },
    });
    await pipeline.runNow();
    expect(pipeline.isIdle()).toBe(true);
    await pipeline.runNow();
    expect(pipeline.isIdle()).toBe(false);
    await vi.advanceTimersByTimeAsync(50);
    expect(reads).toBe(3);
    expect(pipeline.isIdle()).toBe(true);
    pipeline.dispose();
  });

  it('stops after dispose, and invalidate drops runs in flight', async () => {
    openProject();
    const service = manualService();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
      service,
    });
    const run = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    pipeline.invalidate();
    service.calls[0]?.resolve(updateFixture(service.calls[0].json, [diagnosticFixture()]));
    await run;
    expect(useAppStore.getState().analysis.preview).toBeNull();
    expect(useAppStore.getState().project?.textHash).toBe('older');

    pipeline.schedule();
    pipeline.dispose();
    expect(pipeline.pending).toBe(false);
    pipeline.schedule();
    await vi.advanceTimersByTimeAsync(100);
    expect(service.calls).toHaveLength(1);
  });
});
