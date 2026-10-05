/**
 * Which diagnostics the editor shows (docs/spec/04-user-interface.md §4.4): the live preview's, plus
 * the last build's that only a build can find (toolchain, compiler, linker). The build's own
 * analyser diagnostics are left out because the preview reports them already.
 *
 * Compiler messages of the last build stay while the project changes, dimmed and marked "from the
 * last build", for as long as the content hash differs from the build's `projectHash` (M2 decision
 * "Stale compiler diagnostics").
 */
import type { Diagnostic } from '@blocks2cpp/ipc-types';

import { type AppData, initialBuild } from '../../app/store';
import { buildOnlyDiagnostics } from '../../app/store/selectors';

/** The diagnostics to show, from the app's state. */
export interface DiagnosticInputs {
  /** The live preview's diagnostics, in pipeline order. */
  readonly live: readonly Diagnostic[];
  /** The last build's toolchain, compiler, linker (and runtime) diagnostics. */
  readonly build: readonly Diagnostic[];
  /** Whether the project changed since that build: its diagnostics are then dimmed. */
  readonly stale: boolean;
}

const NONE: readonly Diagnostic[] = Object.freeze([]);

/**
 * Whether the last build's diagnostics belong to an older version of the project: the build
 * reported a `projectHash` and the open project's content hash is now different.
 */
export function isBuildStale(state: Pick<AppData, 'build' | 'project'>): boolean {
  return staleHashes(state.build.diagnosticsHash, state.project?.contentHash ?? null);
}

/** Whether a build's `projectHash` (`built`) is not the project's content hash (`current`). */
function staleHashes(built: string | null, current: string | null): boolean {
  return built !== null && current !== null && built !== current;
}

/** The parts of the app's state the diagnostics come from (each compared by reference). */
export interface DiagnosticSources {
  /** The live preview, or null before the first one. */
  readonly preview: AppData['analysis']['preview'];
  /** The last build's diagnostics. */
  readonly buildDiagnostics: AppData['build']['diagnostics'];
  /** The `projectHash` of the last build's diagnostics, or null. */
  readonly diagnosticsHash: string | null;
  /** The open project's content hash, or null without a project. */
  readonly contentHash: string | null;
}

/** The sources of the diagnostics in the app's state. */
export function diagnosticSources(
  state: Pick<AppData, 'analysis' | 'build' | 'project'>,
): DiagnosticSources {
  return {
    preview: state.analysis.preview,
    buildDiagnostics: state.build.diagnostics,
    diagnosticsHash: state.build.diagnosticsHash,
    contentHash: state.project?.contentHash ?? null,
  };
}

/** The live and build diagnostics to show, and whether the build's are stale. */
export function diagnosticInputsFrom(sources: DiagnosticSources): DiagnosticInputs {
  // The shell's rule for "only a build finds these", so Problems' count and rows agree.
  const build =
    sources.buildDiagnostics.length === 0
      ? NONE
      : buildOnlyDiagnostics({
          build: { ...initialBuild(), diagnostics: sources.buildDiagnostics },
        });
  return {
    live: sources.preview?.diagnostics ?? NONE,
    build: build.length === 0 ? NONE : build,
    stale: staleHashes(sources.diagnosticsHash, sources.contentHash),
  };
}

/** The live and build diagnostics to show for the app's state, and whether the build's are stale. */
export function diagnosticInputs(
  state: Pick<AppData, 'analysis' | 'build' | 'project'>,
): DiagnosticInputs {
  return diagnosticInputsFrom(diagnosticSources(state));
}

/**
 * The diagnostics the code panel marks: the live ones and, while the last build matches the
 * project, its compiler messages. Stale ones are left out there, because the panel shows the
 * current code and their ranges would point at code that has changed.
 */
export function codePanelDiagnostics(inputs: DiagnosticInputs): readonly Diagnostic[] {
  if (inputs.stale || inputs.build.length === 0) {
    return inputs.live;
  }
  return [...inputs.live, ...inputs.build];
}
