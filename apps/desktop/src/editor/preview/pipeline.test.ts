/**
 * The live preview pipeline with stand-in cores (docs/spec/06-compiler-pipeline.md §6.13): the
 * debounce, discarding results that newer runs overtook, the dirty state, trap recovery and the
 * notice when the editor's document does not load.
 */
import {
  type BdmDocument,
  type CanonicalResult,
  CoreError,
  CoreTrap,
  type CoreWasm,
  type PreviewOptions,
  type PreviewResult,
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

/** A stand-in core: `canonical` returns the JSON text itself, `preview` a fixture. */
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
      hash: 'b'.repeat(64),
      diagnostics: [],
    }),
    preview: (): PreviewResult => previewFixture(),
    symbolsInScope: () => [],
    conversionTable: unused,
    clipboardMake: unused,
    pastePrepare: unused,
    ...overrides,
  };
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
  calls: { json: string; options: PreviewOptions; resolve: (result: PreviewResult) => void }[];
} {
  const calls: {
    json: string;
    options: PreviewOptions;
    resolve: (result: PreviewResult) => void;
  }[] = [];
  return {
    calls,
    preview: (json, options) =>
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

function openProject(): void {
  useAppStore.getState().actions.setProject(
    projectFixture({
      canonicalText: 'saved',
      savedCanonicalText: JSON.stringify(readDocument()),
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
    const canonical = vi.fn((json: string) => stub.canonical(json));
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore({ canonical })),
      read: readDocument,
    });
    pipeline.schedule();
    await vi.advanceTimersByTimeAsync(30);
    pipeline.schedule();
    await vi.advanceTimersByTimeAsync(30);
    pipeline.schedule();
    expect(pipeline.pending).toBe(true);
    await vi.advanceTimersByTimeAsync(49);
    expect(canonical).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(canonical).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().analysis.preview).not.toBeNull();
    pipeline.dispose();
  });

  it('makes the read document the project, dirty only when it differs from the saved text', async () => {
    openProject();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
    });
    await pipeline.runNow();
    let project = useAppStore.getState().project;
    expect(project?.document.project.name).toBe('Edit 0');
    expect(project?.canonicalText).toBe(JSON.stringify(readDocument()));
    expect(project?.contentHash).toBe('b'.repeat(64));
    expect(project?.dirty).toBe(false);

    docVersion = 1;
    await pipeline.runNow();
    project = useAppStore.getState().project;
    expect(project?.document.project.name).toBe('Edit 1');
    expect(project?.dirty).toBe(true);
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

    const newer = previewFixture([diagnosticFixture({ code: 'B2C-E0203' })]);
    service.calls[1]?.resolve(newer);
    await second;
    service.calls[0]?.resolve(previewFixture([diagnosticFixture({ code: 'B2C-E0201' })]));
    await first;

    expect(useAppStore.getState().analysis.preview).toBe(newer);
    expect(useAppStore.getState().analysis.seq).toBe(2);
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
    service.calls[0]?.resolve(previewFixture([diagnosticFixture()]));
    await run;
    expect(useAppStore.getState().analysis.preview).toBeNull();
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
    const trapping = stubCore({
      preview: () => {
        throw new CoreTrap('preview: unreachable');
      },
    });
    const healthy = stubCore();
    const host = hostOf(trapping, healthy);
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const pipeline = new PreviewPipeline({ store: useAppStore, host, read: readDocument });
    await pipeline.runNow();

    expect(host.restarts).toBe(1);
    expect(host.current()).toBe(healthy);
    const { analysis } = useAppStore.getState();
    expect(analysis.preview).not.toBeNull();
    expect(analysis.notice).toBe('trapRecovered');
    expect(error).toHaveBeenCalled();
    pipeline.dispose();
  });

  it('recovers from a trap in the canonical step too', async () => {
    openProject();
    const trapping = stubCore({
      canonical: () => {
        throw new CoreTrap('canonical: unreachable');
      },
    });
    const host = hostOf(trapping, stubCore());
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const pipeline = new PreviewPipeline({ store: useAppStore, host, read: readDocument });
    await pipeline.runNow();
    expect(host.restarts).toBe(1);
    expect(useAppStore.getState().project?.canonicalText).toBe(JSON.stringify(readDocument()));
    expect(useAppStore.getState().analysis.notice).toBe('trapRecovered');
    pipeline.dispose();
  });

  it('keeps the last good preview when the new core traps as well', async () => {
    openProject();
    const good = previewFixture([diagnosticFixture()]);
    useAppStore.getState().actions.setAnalysis({ preview: good });
    const trapping = stubCore({
      preview: () => {
        throw new CoreTrap('preview: unreachable');
      },
    });
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(trapping, trapping),
      read: readDocument,
    });
    await pipeline.runNow();
    expect(useAppStore.getState().analysis.preview).toBe(good);
    expect(useAppStore.getState().analysis.notice).toBe('trapRecovered');
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
        stubCore({
          canonical: () => ({ ok: false, diagnostics: [diagnosticFixture({ code: 'B2C-E0114' })] }),
        }),
      ),
      read: readDocument,
    });
    await pipeline.runNow();
    const { analysis, project } = useAppStore.getState();
    expect(analysis.preview).toBe(good);
    expect(analysis.notice).toBe('syncFailed');
    // With the loader's codes, for the banner.
    expect(analysis.syncCodes).toEqual(['B2C-E0114']);
    expect(project?.canonicalText).toBe('saved');
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

  it('clears the sync notice as soon as the canvas loads again, even if the preview fails', async () => {
    openProject();
    useAppStore.getState().actions.setAnalysis({ notice: 'syncFailed' });
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(stubCore()),
      read: readDocument,
      service: { preview: () => Promise.reject(new Error('the preview failed')) },
    });
    await pipeline.runNow();
    expect(useAppStore.getState().analysis.notice).toBeNull();
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
      'b2c:preview:canonical',
      'b2c:preview:commit',
      'b2c:preview:analyse',
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
      host: hostOf(stubCore({ canonical: () => ({ ok: false, diagnostics: [] }) })),
      read: readDocument,
    });
    await failing.runNow();
    expect(latestTimings()['b2c:preview']).toBeNull();
    expect(latestTimings()['b2c:preview:canonical']).not.toBeNull();
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
    let loads = true;
    const stub = stubCore();
    const pipeline = new PreviewPipeline({
      store: useAppStore,
      host: hostOf(
        stubCore({
          canonical: (json) =>
            loads
              ? stub.canonical(json)
              : { ok: false, diagnostics: [diagnosticFixture({ code: 'B2C-E0104' })] },
        }),
      ),
      read: readDocument,
      service,
    });
    // Nothing read yet.
    expect(pipeline.isIdle()).toBe(false);

    // A run that has started but not read the canvas yet (the core starts first).
    const first = pipeline.runNow();
    expect(pipeline.isIdle()).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    // Committed: idle, although its analysis still runs.
    expect(service.calls).toHaveLength(1);
    expect(pipeline.isIdle()).toBe(true);
    service.calls[0]?.resolve(previewFixture());
    await first;
    expect(pipeline.isIdle()).toBe(true);

    // An edit waits for its run.
    pipeline.schedule();
    expect(pipeline.isIdle()).toBe(false);
    await vi.advanceTimersByTimeAsync(50);
    expect(pipeline.isIdle()).toBe(true);
    service.calls[1]?.resolve(previewFixture());

    // A new document is shown: what was committed is no longer the canvas.
    pipeline.invalidate();
    expect(pipeline.isIdle()).toBe(false);
    const second = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    service.calls[2]?.resolve(previewFixture());
    await second;
    expect(pipeline.isIdle()).toBe(true);

    // A canvas that does not load keeps the older document: not idle.
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    loads = false;
    await pipeline.runNow();
    expect(useAppStore.getState().analysis.notice).toBe('syncFailed');
    expect(pipeline.isIdle()).toBe(false);
    loads = true;
    const third = pipeline.runNow();
    await vi.advanceTimersByTimeAsync(0);
    service.calls[3]?.resolve(previewFixture());
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
    service.calls[0]?.resolve(previewFixture([diagnosticFixture()]));
    await run;
    expect(useAppStore.getState().analysis.preview).toBeNull();

    pipeline.schedule();
    pipeline.dispose();
    expect(pipeline.pending).toBe(false);
    pipeline.schedule();
    await vi.advanceTimersByTimeAsync(100);
    expect(service.calls).toHaveLength(1);
  });
});
