/**
 * The editor's WebAssembly core (`@blocks2cpp/b2c-core-wasm`) in the frontend build
 * (docs/spec/06-compiler-pipeline.md §6.13). The core's `pkg/` is built from the Rust crates, not
 * committed, so after a `git pull` it can be missing or older than the source, and an app built
 * then would embed C++ generation from the old compiler. The core's build records a fingerprint of
 * its inputs in `pkg/build-info.json`; this plugin compares it with the source:
 *
 * - a production build (`vite build`, in every mode, so also the E2E frontend) stops when the core
 *   is missing, out of date, or has no readable build-info;
 * - the dev server (and Vitest, which runs on it) warns once at start and goes on, so a developer
 *   can still run the editor with an older core;
 * - importing the core's generated modules (`#glue`, `#pkg-bytes`) without a build stops with the
 *   same instructions instead of the bundler's bare "failed to resolve import".
 */
import path from 'node:path';

import {
  BuildInfoError,
  type BuildState,
  checkBuild,
  describeBuild,
} from '@blocks2cpp/b2c-core-wasm/build-info';
import type { Logger, Plugin } from 'vite';

/** Options of {@link wasmCore}. */
export interface WasmCoreOptions {
  /**
   * The repository root whose core is checked. By default two folders above the Vite root
   * (`apps/desktop`).
   */
  repoRoot?: string;
}

/** What is wrong with the core at `repoRoot`, or `null` when it matches the source. */
export function wasmCoreProblem(repoRoot: string): string | null {
  let state: BuildState;
  try {
    state = checkBuild(repoRoot);
  } catch (error: unknown) {
    if (error instanceof BuildInfoError) {
      return `Whether the WebAssembly core of the editor matches the source cannot be checked: ${error.message}.`;
    }
    throw error;
  }
  return describeBuild(state);
}

/** The plugin that checks the core: see the module's documentation. */
export function wasmCore(options: WasmCoreOptions = {}): Plugin {
  let repoRoot = options.repoRoot ?? '';
  let command: 'build' | 'serve' = 'serve';
  let logger: Logger | null = null;

  return {
    name: 'blocks2cpp:wasm-core',
    enforce: 'pre',
    configResolved(config) {
      repoRoot = options.repoRoot ?? path.resolve(config.root, '..', '..');
      command = config.command;
      logger = config.logger;
    },
    buildStart() {
      if (command !== 'build') {
        return;
      }
      const problem = wasmCoreProblem(repoRoot);
      if (problem !== null) {
        this.error(problem);
      }
    },
    configureServer() {
      const problem = wasmCoreProblem(repoRoot);
      if (problem !== null) {
        logger?.warn(
          `\n${problem}\nThe dev server and the tests go on with this core; a production build stops.\n`,
        );
      }
    },
    resolveId: {
      filter: { id: /^#(?:glue|pkg-bytes)$/ },
      async handler(source, importer, resolveOptions) {
        const resolved = await this.resolve(source, importer, {
          ...resolveOptions,
          skipSelf: true,
        });
        if (resolved === null) {
          this.error(
            `${describeBuild({ state: 'notBuilt' }) ?? ''} (${source} points into the core's pkg/.)`,
          );
        }
        return resolved;
      },
    },
  };
}
