/**
 * Values derived from the app's state. Each one is a plain function of the state, so it can be
 * used in a selector (`useAppStore(selectedToolchain)`) or on `getState()`.
 */
import type { Diagnostic, DiagSource, Toolchain } from '@blocks2cpp/ipc-types';

import type { AppData } from './state';

/**
 * Diagnostic sources the live preview never reports: only a build finds these, so the Problems
 * count adds them to the preview's own diagnostics.
 */
const BUILD_ONLY_SOURCES: ReadonlySet<DiagSource> = new Set<DiagSource>([
  'toolchain',
  'compiler',
  'linker',
  'runtime',
]);

/**
 * The toolchain a build would use: the selected one when it is usable, otherwise the first usable
 * one in discovery order (as the backend falls back, B2C-T1022), or `null` when none is usable.
 */
export function selectedToolchain(state: Pick<AppData, 'toolchains'>): Toolchain | null {
  const { list } = state.toolchains;
  return (
    list.find((toolchain) => toolchain.selected && toolchain.usable) ??
    list.find((toolchain) => toolchain.usable) ??
    null
  );
}

/** Whether at least one toolchain can build. */
export function hasUsableToolchain(state: Pick<AppData, 'toolchains'>): boolean {
  return state.toolchains.list.some((toolchain) => toolchain.usable);
}

/**
 * The errors the live preview reports for the open project (loader, catalog, analyser and
 * generator): the ones the backend refuses to build with (07 §7.6.1).
 */
export function liveErrors(state: Pick<AppData, 'analysis'>): Diagnostic[] {
  const diagnostics = state.analysis.preview?.diagnostics ?? [];
  return diagnostics.filter((diagnostic) => diagnostic.severity === 'error');
}

/** The build's diagnostics that the live preview cannot report itself. */
export function buildOnlyDiagnostics(state: Pick<AppData, 'build'>): Diagnostic[] {
  return state.build.diagnostics.filter((diagnostic) => BUILD_ONLY_SOURCES.has(diagnostic.source));
}

/** How many entries the Problems tab lists: the live diagnostics plus the build-only ones. */
export function problemCount(state: Pick<AppData, 'analysis' | 'build'>): number {
  return (state.analysis.preview?.diagnostics.length ?? 0) + buildOnlyDiagnostics(state).length;
}

/** The first error to show when the user asks for it: a live one first, then a build one. */
export function firstError(state: Pick<AppData, 'analysis' | 'build'>): Diagnostic | null {
  return (
    liveErrors(state)[0] ??
    buildOnlyDiagnostics(state).find((diagnostic) => diagnostic.severity === 'error') ??
    null
  );
}
