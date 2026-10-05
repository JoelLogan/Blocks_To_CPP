/**
 * Starting, publishing and replacing the compiler core (`@blocks2cpp/b2c-core-wasm`, 06 §6.13).
 * The editor starts it once, publishes it with the shell's `setCore` (features read it through
 * `FeatureContext.core()`), and replaces it after a trap (`CoreTrap`): the trapped instance is never
 * called again.
 */
import {
  CoreError,
  type CoreWasm,
  initCore as packageInitCore,
  resetCore as packageResetCore,
} from '@blocks2cpp/b2c-core-wasm';

import { getCore, setCore } from '../../app/core';

/** Where the editor gets the compiler core. */
export interface CoreHost {
  /** The running core, or `null` before it has started. */
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
    current: () => deps.getCore(),
    start,
    restart: () => {
      deps.resetCore();
      deps.setCore(null);
      starting = null;
      return start();
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
    preview: (documentJson, options) => running().preview(documentJson, options),
    symbolsInScope: (blockId, input) => running().symbolsInScope(blockId, input),
    conversionTable: () => running().conversionTable(),
    clipboardMake: (documentJson, blockIds) => running().clipboardMake(documentJson, blockIds),
    pastePrepare: (clipboardText, documentJson, target, seedHex) =>
      running().pastePrepare(clipboardText, documentJson, target, seedHex),
  };
}
