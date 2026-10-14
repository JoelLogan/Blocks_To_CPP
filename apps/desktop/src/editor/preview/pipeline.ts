/**
 * The live preview pipeline (02 §2.4.1, 06 §6.13). After the editor's changes settle (a 50 ms
 * debounce), it reads the document from the canvas, serialises it and runs the compiler core's
 * `update()` with the machine's indent width: one call, in which the core loads the document (the
 * loader validates it), hashes it and previews it. Then
 *
 * 1. the document becomes the project's, with its content hash and the hash of its canonical text
 *    (`textHash`); the project has unsaved changes (`dirty`) when that differs from the hash of
 *    the saved text (05 §5.2). The canonical text itself is not made per edit: saving, building
 *    and autosave ask the core for it (`canonical()`) when they need it;
 * 2. the preview goes into `analysis.preview` (diagnostics, generated C++, source map, symbols,
 *    block types).
 *
 * Each step is one store update (the project with the run's sequence number, then the preview),
 * so the store's subscribers hear of a run at most twice. Each run and each of its phases is a
 * User Timing span (`b2c:preview`, `b2c:preview:read`, `b2c:preview:update`, …,
 * src/lib/timing.ts).
 *
 * A sequence number discards results that a newer run has overtaken. When the core traps, the
 * instance is replaced, the run is tried once more on the new one, and the analysis gets the
 * notice `trapRecovered`. When the update fails even so (the new instance trapped too, or the core
 * failed in another way), the document is still made the project's through `canonical()` alone
 * (on a fresh instance after a trap), and the last good preview stays: an edit the analysis cannot
 * handle still counts as unsaved, and autosave still keeps it. When the document read from the
 * canvas does not load (blocks nested deeper than a project may be, B2C-E0104, or a bug in the
 * sync), nothing is committed: the project and the last good preview stay, and the analysis gets
 * the notice `syncFailed` (with the loader's codes) until the canvas loads again. The window shows
 * both notices in a banner (src/features/analysis), and Build and Run refuse such a canvas instead
 * of building the older document (src/features/build-run).
 */
import {
  type BdmDocument,
  type CanonicalResult,
  CoreTrap,
  type CoreWasm,
  type Diagnostic,
  type PreviewOptions,
  type PreviewResult,
  type UpdateResult,
} from '@blocks2cpp/b2c-core-wasm';

import type { AnalysisNotice, useAppStore } from '../../app/store';
import { startSpan, timed } from '../../lib/timing';
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

/** The hashes of a document that loaded: what the project keeps of it besides the document. */
interface DocumentHashes {
  /** The SHA-256 of its canonical text, which `dirty` compares. */
  readonly textHash: string;
  /** Its content hash (05 §5.11). */
  readonly contentHash: string;
}

/** The document read for a run, and the JSON text the core was given. */
interface ReadDocument {
  readonly doc: BdmDocument;
  readonly json: string;
}

/** The hashes of an update whose document loaded, or `null` when it did not load. */
function hashesOf(result: UpdateResult): DocumentHashes | null {
  if (result.stage === 'load' || result.textHash === null || result.contentHash === null) {
    return null;
  }
  return { textHash: result.textHash, contentHash: result.contentHash };
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
    let read: ReadDocument | null = null;
    try {
      await this.core();
      check();
      const run = startSpan('b2c:preview');
      const reading = startSpan('b2c:preview:read');
      const doc = this.options.read();
      if (doc === null) {
        return;
      }
      const json = JSON.stringify(doc);
      read = { doc, json };
      reading.end();

      const previewOptions: PreviewOptions = { indentWidth: this.indentWidth() };
      const update = startSpan('b2c:preview:update');
      let result: UpdateResult;
      try {
        result = await this.service.update(json, previewOptions);
      } catch (error: unknown) {
        if (!(error instanceof CoreTrap)) {
          throw error;
        }
        trapped = true;
        await this.recover(error);
        check();
        result = await this.service.update(json, previewOptions);
      }
      check();
      update.end();
      const hashes = hashesOf(result);
      if (hashes === null) {
        this.syncFailed(result.diagnostics);
        return;
      }
      timed('b2c:preview:commit', () => {
        this.commit(seq, doc, hashes);
      });
      timed('b2c:preview:publish', () => {
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
      });
      run.end();
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
      } else {
        console.error('The live preview failed', error);
      }
      if (read !== null) {
        await this.commitWithoutPreview(seq, read, check);
      }
    }
  }

  /**
   * After an update that failed: makes the read document the project's through the core's
   * `canonical()` alone (on a fresh instance when the current one trapped), so an edit the
   * analysis cannot handle still counts as unsaved and autosave still keeps it. The last good
   * preview stays. When this fails too, the project stays as it was and the failure is logged.
   */
  private async commitWithoutPreview(
    seq: number,
    read: ReadDocument,
    check: () => void,
  ): Promise<void> {
    try {
      let core = await this.core();
      check();
      let canonical: CanonicalResult;
      try {
        canonical = core.canonical(read.json);
      } catch (error: unknown) {
        if (!(error instanceof CoreTrap)) {
          throw error;
        }
        core = await this.recover(error);
        check();
        canonical = core.canonical(read.json);
      }
      if (canonical.ok) {
        this.commit(seq, read.doc, { textHash: canonical.textHash, contentHash: canonical.hash });
      } else {
        this.syncFailed(canonical.diagnostics);
      }
    } catch (error: unknown) {
      if (!(error instanceof Abandoned)) {
        console.error('The edit could not be kept without a preview', error);
      }
    }
  }

  /**
   * The document read from the canvas did not load: nothing is committed, and the analysis gets
   * the notice `syncFailed` with the loader's codes, so the banner can say which limit the canvas
   * went past. The log names the codes only, never project text.
   */
  private syncFailed(diagnostics: readonly Diagnostic[]): void {
    const codes = diagnostics.map((diagnostic) => diagnostic.code);
    console.error(`The editor's document did not load (${codes.join(', ')}).`);
    this.options.store.getState().actions.setAnalysis({ notice: 'syncFailed', syncCodes: codes });
  }

  /**
   * Makes `doc` the open project's document, with its hashes (it has unsaved changes when the
   * hash of its canonical text differs from the saved text's), and records run `seq` as the
   * latest preview request, in one store update. A canvas that loads again clears the
   * `syncFailed` notice: what the project holds is what it shows.
   */
  private commit(seq: number, doc: BdmDocument, hashes: DocumentHashes): void {
    const { project, analysis, actions } = this.options.store.getState();
    if (project === null) {
      return;
    }
    actions.updateProjectAndAnalysis(
      {
        document: doc,
        textHash: hashes.textHash,
        contentHash: hashes.contentHash,
        dirty: hashes.textHash !== project.savedTextHash,
      },
      analysis.notice === 'syncFailed' ? { seq, notice: null } : { seq },
    );
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
