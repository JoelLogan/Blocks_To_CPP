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
