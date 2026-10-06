/**
 * The live preview pipeline (02 §2.4.1, 06 §6.13). After the editor's changes settle (a 50 ms
 * debounce), it reads the document from the canvas and
 *
 * 1. serialises it and runs the compiler core's `canonical()`: the loader validates it, and its
 *    canonical text and hash become the project's (`dirty` when the text differs from the last
 *    saved text, 05 §5.2);
 * 2. runs the core's `preview()` with the machine's indent width, and puts the result in
 *    `analysis.preview` (diagnostics, generated C++, source map, symbols, block types).
 *
 * A sequence number discards results that a newer run has overtaken. When the core traps, the
 * instance is replaced, the run is tried once more on the new one, and the analysis gets the
 * notice `trapRecovered`. When the document read from the canvas does not load (blocks nested
 * deeper than a project may be, B2C-E0104, or a bug in the sync), nothing is committed: the
 * project and the last good preview stay, and the analysis gets the notice `syncFailed` until the
 * canvas loads again. The window shows both notices in a banner (src/features/analysis), and Build
 * and Run refuse such a canvas instead of building the older document (src/features/build-run).
 */
import {
  type BdmDocument,
  type CanonicalResult,
  CoreTrap,
  type CoreWasm,
  type PreviewOptions,
  type PreviewResult,
} from '@blocks2cpp/b2c-core-wasm';

import type { AnalysisNotice, useAppStore } from '../../app/store';
import type { CoreHost } from './coreHost';
import { mainThreadPreviewService, type PreviewService } from './service';

/** How long the pipeline waits after the last change before it runs (02 §2.4.1). */
export const PREVIEW_DEBOUNCE_MS = 50;

/** The indent width when the settings have not been read. */
const DEFAULT_INDENT_WIDTH = 4;

/** What the pipeline works with. */
export interface PreviewPipelineOptions {
  readonly store: typeof useAppStore;
  readonly host: CoreHost;
  /** Reads the open project's document from the canvas, or `null` when there is nothing to read. */
  readonly read: () => BdmDocument | null;
  /** Runs the preview; the main-thread service by default. */
  readonly service?: PreviewService;
  /** Called after a document has been made the project's. */
  readonly onCommitted?: (doc: BdmDocument) => void;
  /** Called after a preview has been put in the store. */
  readonly onPreviewed?: (result: PreviewResult) => void;
  /** The debounce delay, in milliseconds. */
  readonly debounceMs?: number;
}

/** The run was overtaken, the project closed, or the core could not be used: stop quietly. */
class Abandoned extends Error {
  override readonly name = 'Abandoned';
}

/** The diagnostics codes of a failed load, for the log (never project text). */
function codesOf(result: Extract<CanonicalResult, { ok: false }>): string {
  return result.diagnostics.map((diagnostic) => diagnostic.code).join(', ');
}

/** The debounced, sequenced preview pipeline of one editor. */
export class PreviewPipeline {
  private readonly options: PreviewPipelineOptions;
  private readonly service: PreviewService;
  private readonly debounceMs: number;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private latest = 0;
  private disposed = false;
  private readonly unsubscribe: () => void;

  constructor(options: PreviewPipelineOptions) {
    this.options = options;
    this.service = options.service ?? mainThreadPreviewService(options.host);
    this.debounceMs = options.debounceMs ?? PREVIEW_DEBOUNCE_MS;
    // A different indent width changes the generated code: preview again.
    this.unsubscribe = options.store.subscribe((state, previous) => {
      const width = state.settings.value?.codeStyle.indentWidth;
      if (width !== previous.settings.value?.codeStyle.indentWidth && state.project !== null) {
        this.schedule();
      }
    });
  }

  /** Whether a debounced run is waiting. */
  get pending(): boolean {
    return this.timer !== null;
  }

  /** Runs after the debounce delay; another call before then restarts the delay. */
  schedule(): void {
    if (this.disposed) {
      return;
    }
    this.cancel();
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.run();
    }, this.debounceMs);
  }

  /** Runs now (a waiting run is cancelled). Resolves when the run has finished or was dropped. */
  runNow(): Promise<void> {
    this.cancel();
    return this.run();
  }

  /** Cancels a waiting run. */
  cancel(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  /** Discards the results of runs in progress (a new document is shown). */
  invalidate(): void {
    this.cancel();
    this.latest += 1;
  }

  /** Stops for good. */
  dispose(): void {
    this.invalidate();
    this.disposed = true;
    this.unsubscribe();
  }

  private async run(): Promise<void> {
    if (this.disposed) {
      return;
    }
    const seq = ++this.latest;
    const handle = this.options.store.getState().project?.handle;
    if (handle === undefined) {
      return;
    }
    const check = (): void => {
      if (
        this.disposed ||
        seq !== this.latest ||
        this.options.store.getState().project?.handle !== handle
      ) {
        throw new Abandoned();
      }
    };
    let trapped = false;
    try {
      let core = await this.core();
      check();
      const doc = this.options.read();
      if (doc === null) {
        return;
      }
      const json = JSON.stringify(doc);
      let canonical: CanonicalResult;
      try {
        canonical = core.canonical(json);
      } catch (error: unknown) {
        if (!(error instanceof CoreTrap)) {
          throw error;
        }
        trapped = true;
        core = await this.recover(error);
        check();
        canonical = core.canonical(json);
      }
      if (!canonical.ok) {
        console.error(`The editor's document did not load (${codesOf(canonical)}).`);
        this.setNotice('syncFailed');
        return;
      }
      this.commit(doc, canonical);
      if (this.options.store.getState().analysis.notice === 'syncFailed') {
        // The canvas loads again: what the project holds is what it shows.
        this.options.store.getState().actions.setAnalysis({ notice: null });
      }

      this.options.store.getState().actions.setAnalysis({ seq });
      const previewOptions: PreviewOptions = { indentWidth: this.indentWidth() };
      let result: PreviewResult;
      try {
        result = await this.service.preview(json, previewOptions);
      } catch (error: unknown) {
        if (!(error instanceof CoreTrap)) {
          throw error;
        }
        trapped = true;
        await this.recover(error);
        check();
        result = await this.service.preview(json, previewOptions);
      }
      check();
      const { analysis, actions } = this.options.store.getState();
      actions.setAnalysis({
        preview: result,
        notice: trapped
          ? 'trapRecovered'
          : analysis.notice === 'syncFailed'
            ? null
            : analysis.notice,
      });
      this.options.onPreviewed?.(result);
    } catch (error: unknown) {
      if (error instanceof Abandoned) {
        return;
      }
      if (error instanceof CoreTrap) {
        // The fresh instance trapped too: keep the last good preview.
        console.error('The compiler core stopped again after a restart', error);
        if (seq === this.latest) {
          this.setNotice('trapRecovered');
        }
        return;
      }
      console.error('The live preview failed', error);
    }
  }

  /** Makes `doc` the open project's document, with its canonical text and hash. */
  private commit(doc: BdmDocument, canonical: Extract<CanonicalResult, { ok: true }>): void {
    const { project, actions } = this.options.store.getState();
    if (project === null) {
      return;
    }
    actions.updateProject({
      document: doc,
      canonicalText: canonical.text,
      contentHash: canonical.hash,
      dirty: canonical.text !== project.savedCanonicalText,
    });
    this.options.onCommitted?.(doc);
  }

  /** The running core, starting it if needed. */
  private async core(): Promise<CoreWasm> {
    try {
      return this.options.host.current() ?? (await this.options.host.start());
    } catch (error: unknown) {
      console.error('The compiler core could not be started', error);
      throw new Abandoned();
    }
  }

  /** Replaces a trapped core. */
  private async recover(trap: CoreTrap): Promise<CoreWasm> {
    console.error('The compiler core stopped; starting a new one', trap);
    try {
      return await this.options.host.restart();
    } catch (error: unknown) {
      console.error('The compiler core could not be restarted', error);
      this.setNotice('trapRecovered');
      throw new Abandoned();
    }
  }

  private setNotice(notice: AnalysisNotice): void {
    this.options.store.getState().actions.setAnalysis({ notice });
  }

  private indentWidth(): 2 | 4 {
    return (
      this.options.store.getState().settings.value?.codeStyle.indentWidth ?? DEFAULT_INDENT_WIDTH
    );
  }
}
