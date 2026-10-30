/**
 * Starting, publishing and replacing the compiler core (`@blocks2cpp/b2c-core-wasm`, 06 §6.13).
 * The editor starts it once, publishes it with the shell's `setCore` (features read it through
 * `FeatureContext.core()`), and replaces it after a trap (`CoreTrap`): the trapped instance is never
 * called again.
 *
 * While the preview runs in its worker (ADR-0016), the main instance no longer previews, so its
 * own scope query would answer about a document it has not seen. The preview pipeline then gives
 * the host the scope index of the preview it applied ({@link CoreHost.applyScope}), and the
 * editor's instance ({@link CoreHost.current}) answers `symbolsInScope` from it
 * (docs/plans/m3-contracts/w5-performance.md §2.2). Without an index (a preview made on the main
 * thread, whose instance holds that very analysis) the instance answers itself, as in M2. With
 * {@link NO_SCOPE} (the worker previews, but there is no index for the document shown: another
 * document was loaded, or the worker's index could not be read) it answers nothing, so a menu falls
 * back to the block's place rather than ask the main instance about a document it has not seen.
 */
import {
  CoreError,
  type CoreWasm,
  type ScopeIndex,
  initCore as packageInitCore,
  resetCore as packageResetCore,
} from '@blocks2cpp/b2c-core-wasm';

import { getCore, setCore } from '../../app/core';

/** What the scope query answers from while the worker previews and no index fits ({@link CoreHost.applyScope}). */
export const NO_SCOPE = 'none';

/** Where the editor gets the compiler core. */
export interface CoreHost {
  /**
   * The running core, or `null` before it has started. While a scope index is applied, its
   * `symbolsInScope` answers from that index; every other call is the instance's.
   */
  current(): CoreWasm | null;
  /**
   * The running core, starting it on first use.
   *
   * @throws CoreError (`init`) when the module cannot be started.
   */
  start(): Promise<CoreWasm>;
  /**
   * Replaces a trapped core with a fresh instance and publishes it.
   *
   * @throws CoreError (`init`) when no new instance can be started.
   */
  restart(): Promise<CoreWasm>;
  /**
   * Makes `scope` the index the scope query answers from (the applied preview's, made in the
   * worker); with `null` lets the instance answer (a preview made on the main thread); with
   * {@link NO_SCOPE} answers nothing (`null`, no answer).
   */
  applyScope?(scope: ScopeIndex | typeof NO_SCOPE | null): void;
}

/** What a {@link CoreHost} is built from (replaceable in tests). */
export interface CoreHostDeps {
  readonly initCore: () => Promise<CoreWasm>;
  readonly resetCore: () => void;
  readonly getCore: () => CoreWasm | null;
  readonly setCore: (core: CoreWasm | null) => void;
}

const PACKAGE_DEPS: CoreHostDeps = {
  initCore: packageInitCore,
  resetCore: packageResetCore,
  getCore,
  setCore,
};

/** A core host over `deps` (by default the package's `initCore` and the shell's `setCore`). */
export function createCoreHost(deps: CoreHostDeps = PACKAGE_DEPS): CoreHost {
  let starting: Promise<CoreWasm> | null = null;
  let scope: ScopeIndex | typeof NO_SCOPE | null = null;
  /** The view of each instance that answers the scope query from the applied index. */
  const views = new WeakMap<CoreWasm, CoreWasm>();

  const viewOf = (core: CoreWasm): CoreWasm => {
    let view = views.get(core);
    if (view === undefined) {
      view = {
        version: () => core.version(),
        load: (bytes) => core.load(bytes),
        canonical: (documentJson) => core.canonical(documentJson),
        update: (documentJson, options) => core.update(documentJson, options),
        symbolsInScope: (blockId, input) =>
          scope === null
            ? core.symbolsInScope(blockId, input)
            : scope === NO_SCOPE
              ? null
              : scope.symbolsAt(blockId, input),
        conversionTable: () => core.conversionTable(),
        clipboardMake: (documentJson, blockIds) => core.clipboardMake(documentJson, blockIds),
        pastePrepare: (clipboardText, documentJson, target, seedHex) =>
          core.pastePrepare(clipboardText, documentJson, target, seedHex),
      };
      views.set(core, view);
    }
    return view;
  };

  function start(): Promise<CoreWasm> {
    const running = deps.getCore();
    if (running !== null) {
      return Promise.resolve(running);
    }
    if (starting === null) {
      const attempt = deps.initCore().then((core) => {
        deps.setCore(core);
        return core;
      });
      starting = attempt;
      // A failed start is not remembered, so the next call tries again.
      attempt.then(
        () => {
          if (starting === attempt) {
            starting = null;
          }
        },
        () => {
          if (starting === attempt) {
            starting = null;
          }
        },
      );
    }
    return starting;
  }

  return {
    current: () => {
      const core = deps.getCore();
      return core === null || scope === null ? core : viewOf(core);
    },
    start,
    restart: () => {
      deps.resetCore();
      deps.setCore(null);
      starting = null;
      return start();
    },
    applyScope: (index) => {
      scope = index;
    },
  };
}

let shared: CoreHost | null = null;

/** The app's core host (the package's module and the shell's `setCore`). */
export function appCoreHost(): CoreHost {
  shared ??= createCoreHost();
  return shared;
}

/**
 * A `CoreWasm` that always calls the host's current instance, so a holder (an editor plugin) keeps
 * working after a trap replaced the instance.
 *
 * @throws CoreError (`init`) from every method while no core is running.
 */
export function liveCore(host: CoreHost): CoreWasm {
  const running = (): CoreWasm => {
    const core = host.current();
    if (core === null) {
      throw new CoreError('init', 'The compiler core has not started yet.');
    }
    return core;
  };
  return {
    version: () => running().version(),
    load: (bytes) => running().load(bytes),
    canonical: (documentJson) => running().canonical(documentJson),
    update: (documentJson, options) => running().update(documentJson, options),
    symbolsInScope: (blockId, input) => running().symbolsInScope(blockId, input),
    conversionTable: () => running().conversionTable(),
    clipboardMake: (documentJson, blockIds) => running().clipboardMake(documentJson, blockIds),
    pastePrepare: (clipboardText, documentJson, target, seedHex) =>
      running().pastePrepare(clipboardText, documentJson, target, seedHex),
  };
}
