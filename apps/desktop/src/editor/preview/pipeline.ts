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
 * Sequence numbers order the runs (docs/plans/m3-contracts/worker.md §6 and w5-performance.md
 * §2.3). A run that has not read the canvas yet stops when a newer one starts. A run's answer is
 * applied when no newer run's answer has been, even while a newer run waits for its own: with the
 * preview worker, edits less than one `update()` apart would otherwise drop every answer until the
 * typing stops. `invalidate()` (a new document shown) drops every answer of the runs before it.
 * An answer is also dropped when something else (a rename, an adopted save) wrote the project's
 * document after the run read the canvas: it was computed for a document the project no longer
 * holds, so the canvas is read again instead (ADR-0016 §5); a document the pipeline's own last
 * commit wrote does not count. On the main thread the read, `update()` and the commit are one
 * task and neither happens; with the worker they can. While a block is dragged (`isBusy`), an
 * answer that may be applied is held, the newest replacing an older one, and applied once the
 * drag has ended (checked every animation frame), by the same rules. When the
 * core traps, the service gets a fresh instance (`PreviewService.recover`), the run is tried once
 * more on it, and the analysis gets the notice `trapRecovered`. When the update fails even so (the
 * new instance trapped too, or the core failed in another way), the document is still made the
 * project's through `canonical()` alone (on a fresh instance after a trap), and the last good
 * preview stays: an edit the analysis cannot handle still counts as unsaved, and autosave still
 * keeps it. When the document read from the canvas does not load (blocks nested deeper than a
 * project may be, B2C-E0104, or a bug in the sync), nothing is committed: the project and the last
 * good preview stay, and the analysis gets the notice `syncFailed` (with the loader's codes) until
 * the canvas loads again. The window shows both notices in a banner (src/features/analysis), and
 * Build and Run refuse such a canvas instead of building the older document
 * (src/features/build-run).
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
import { recordSpan, startSpan, timed } from '../../lib/timing';
import type { CoreHost } from './coreHost';
import { mainThreadPreviewService, type PreviewAnswer, type PreviewService } from './service';

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
  /**
   * Whether answers must wait (a block is being dragged); never by default. A held answer is
   * applied once this is false again, checked every animation frame.
   */
  readonly isBusy?: () => boolean;
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

/** The document read for a run, the JSON text the core was given, and what it was read against. */
interface ReadDocument {
  readonly doc: BdmDocument;
  readonly json: string;
  /** The project's document when the run read the canvas. */
  readonly base: BdmDocument | null;
}

/** An answer that may be applied, waiting for the end of a drag. */
interface HeldAnswer {
  readonly seq: number;
  readonly apply: () => void;
}

/** Calls `callback` at the next animation frame, or after about one frame without them. */
function nextFrame(callback: () => void): () => void {
  if (typeof requestAnimationFrame === 'function') {
    const id = requestAnimationFrame(callback);
    return () => {
      cancelAnimationFrame(id);
    };
  }
  const id = setTimeout(callback, 16);
  return () => {
    clearTimeout(id);
  };
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
  /** The sequence number of the run whose read of the canvas was committed last; -1 before any. */
  private committed = -1;
  /**
   * The sequence number of the newest run whose answer was applied (committed, or reported as a
   * canvas that does not load); an older answer is never applied after it. -1 before any.
   */
  private settled = -1;
  /** The document the pipeline's own last commit wrote, or `null`. */
  private written: BdmDocument | null = null;
  /** Bumped by `invalidate()`: answers of runs before it are dropped. */
  private epoch = 0;
  /** The answer held while `isBusy()`, or `null`. */
  private held: HeldAnswer | null = null;
  /** Cancels the frame callback that looks at the held answer again, or `null`. */
  private cancelHold: (() => void) | null = null;
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

  /**
   * Whether the pipeline has nothing left to read: no run waits or is still running, and the
   * latest run's read loaded and is the project's document (a run commits it when its `update()`
   * answers). Nothing has asked for another read since then (an edit of the canvas, a new
   * document, a rename), so the project's document holds what reading the canvas would give now,
   * except for a change Blockly has made but not announced yet: Blockly delivers its events after
   * the next animation frame, and only then is a run scheduled. A canvas that does not load
   * (`syncFailed`) is never idle.
   */
  isIdle(): boolean {
    return this.timer === null && this.committed === this.latest && this.held === null;
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
    this.epoch += 1;
    this.dropHeld();
    this.options.host.applyScope?.(null);
  }

  /** Stops for good. */
  dispose(): void {
    this.invalidate();
    this.disposed = true;
    this.unsubscribe();
  }

  private dropHeld(): void {
    this.held = null;
    this.cancelHold?.();
    this.cancelHold = null;
  }

  /** Holds `answer` while `isBusy()`, replacing an older held one; applies it once not busy. */
  private hold(answer: HeldAnswer): void {
    if (this.held !== null && this.held.seq > answer.seq) {
      return;
    }
    this.held = answer;
    if (this.cancelHold !== null) {
      return;
    }
    const look = (): void => {
      this.cancelHold = null;
      const held = this.held;
      if (held === null || this.disposed) {
        return;
      }
      if (this.options.isBusy?.() === true) {
        this.cancelHold = nextFrame(look);
        return;
      }
      this.held = null;
      held.apply();
    };
    this.cancelHold = nextFrame(look);
  }

  private async run(): Promise<void> {
    if (this.disposed) {
      return;
    }
    const seq = ++this.latest;
    const epoch = this.epoch;
    const handle = this.options.store.getState().project?.handle;
    if (handle === undefined) {
      return;
    }
    /** Before the read and before a retry: stop when a newer run has started. */
    const check = (): void => {
      if (
        this.disposed ||
        seq !== this.latest ||
        this.options.store.getState().project?.handle !== handle
      ) {
        throw new Abandoned();
      }
    };
    /** Before applying an answer: stop when it can no longer be applied. */
    const checkApply = (): void => {
      if (
        this.disposed ||
        epoch !== this.epoch ||
        seq <= this.settled ||
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
      // The project's document this run reads against.
      const base = this.options.store.getState().project?.document ?? null;
      const doc = this.options.read();
      if (doc === null) {
        return;
      }
      const json = JSON.stringify(doc);
      read = { doc, json, base };
      reading.end();

      const previewOptions: PreviewOptions = { indentWidth: this.indentWidth() };
      const wait = startSpan('b2c:preview:wait');
      let answer: PreviewAnswer | null;
      try {
        answer = await this.service.update(json, previewOptions);
      } catch (error: unknown) {
        if (!(error instanceof CoreTrap)) {
          throw error;
        }
        trapped = true;
        await this.recoverService(error);
        check();
        answer = await this.service.update(json, previewOptions);
      }
      if (answer === null) {
        // A newer run's request replaced this one.
        throw new Abandoned();
      }
      wait.end();
      const { timing } = answer;
      recordSpan('b2c:preview:update', timing.startedAt, timing.startedAt + timing.updateMs);
      if (answer.scope !== null) {
        const indexed = timing.startedAt + timing.updateMs;
        recordSpan('b2c:preview:index', indexed, indexed + timing.indexMs);
      }
      checkApply();
      this.checkBase(seq, read);
      const accepted = answer;
      const readDoc = read;
      const apply = (): void => {
        if (this.applyAnswer(seq, readDoc, accepted, trapped)) {
          run.end();
        }
      };
      if (this.options.isBusy?.() === true) {
        this.hold({
          seq,
          apply: () => {
            try {
              checkApply();
              this.checkBase(seq, readDoc);
              apply();
            } catch (error: unknown) {
              if (!(error instanceof Abandoned)) {
                console.error('The live preview failed', error);
              }
            }
          },
        });
        return;
      }
      apply();
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
      // A run that failed after its commit (`onCommitted`, or a hook or a store subscriber that
      // throws while the preview is published) has its document committed already: committing it
      // again would trip the base check on its own write and read the canvas forever.
      if (read !== null && this.committed !== seq) {
        await this.commitWithoutPreview(seq, read, check);
      }
    }
  }

  /**
   * Applies an answer that passed the checks: commits its document and publishes its preview.
   * Returns whether there was a preview (a document that does not load has none).
   */
  private applyAnswer(
    seq: number,
    read: ReadDocument,
    answer: PreviewAnswer,
    trapped: boolean,
  ): boolean {
    const result: UpdateResult = answer.result;
    const hashes = hashesOf(result);
    if (hashes === null) {
      this.settled = seq;
      this.syncFailed(result.diagnostics);
      return false;
    }
    timed('b2c:preview:commit', () => {
      this.commit(seq, read.doc, hashes);
    });
    timed('b2c:preview:publish', () => {
      const { analysis, actions } = this.options.store.getState();
      // The index first, so a subscriber that builds a menu at once reads the new one.
      this.options.host.applyScope?.(answer.scope);
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
    return true;
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
      this.checkBase(seq, read);
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
    // Before the store update, whose subscribers may throw after the document is written.
    this.committed = seq;
    this.settled = Math.max(this.settled, seq);
    this.written = doc;
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

  /**
   * Drops a run's answer (throws {@link Abandoned}) when the project's document is no longer the
   * one the run read against, nor the one the pipeline's own last commit wrote (an older run's,
   * applied while this one waited): something else (a rename, an adopted save) wrote it since, and
   * committing the run's document would undo that. When the run is the latest, the canvas is read
   * again. A run's own commit would trip it too, so it is checked only before that commit: a run
   * that fails after committing does not commit again (see the end of `run`).
   */
  private checkBase(seq: number, read: ReadDocument): void {
    const current = this.options.store.getState().project?.document ?? null;
    if (current === read.base || (current !== null && current === this.written)) {
      return;
    }
    if (seq === this.latest) {
      this.schedule();
    }
    throw new Abandoned();
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

  /** Gets the service a fresh instance after a trap in `update()`. */
  private async recoverService(trap: CoreTrap): Promise<void> {
    console.error('The compiler core stopped; starting a new one', trap);
    try {
      await this.service.recover();
    } catch (error: unknown) {
      console.error('The compiler core could not be restarted', error);
      this.setNotice('trapRecovered');
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
