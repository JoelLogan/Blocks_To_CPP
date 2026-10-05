/**
 * Which diagnostics the editor shows: the live preview's, the build's that only a build finds, and
 * whether the build's are stale.
 */
import type { Diagnostic } from '@blocks2cpp/ipc-types';
import { describe, expect, it } from 'vitest';

import { initialAppData } from '../../app/store';
import { diagnosticFixture, previewFixture, projectFixture } from '../../app/testing/fixtures';
import { codePanelDiagnostics, diagnosticInputs, isBuildStale } from './inputs';

const HASH = 'a'.repeat(64);

function state(build: Diagnostic[], buildHash: string | null, contentHash: string | null) {
  const data = initialAppData();
  return {
    ...data,
    project: contentHash === null ? null : projectFixture({ contentHash }),
    analysis: { ...data.analysis, preview: previewFixture([diagnosticFixture()]) },
    build: { ...data.build, diagnostics: build, diagnosticsHash: buildHash },
  };
}

const COMPILER: Diagnostic = diagnosticFixture({ code: 'C:error', source: 'compiler' });
const LINKER: Diagnostic = diagnosticFixture({ code: 'C:link', source: 'linker' });
const ANALYSER: Diagnostic = diagnosticFixture({ source: 'analyser' });

describe('diagnosticInputs', () => {
  it('takes the live preview’s and only the build’s compiler, linker and toolchain ones', () => {
    const inputs = diagnosticInputs(state([ANALYSER, COMPILER, LINKER], HASH, HASH));
    expect(inputs.live).toHaveLength(1);
    expect(inputs.build).toEqual([COMPILER, LINKER]);
    expect(inputs.stale).toBe(false);
  });

  it('calls the build’s stale only when both hashes are known and differ', () => {
    expect(isBuildStale(state([COMPILER], HASH, 'b'.repeat(64)))).toBe(true);
    expect(isBuildStale(state([COMPILER], HASH, HASH))).toBe(false);
    expect(isBuildStale(state([COMPILER], null, HASH))).toBe(false);
    expect(isBuildStale(state([COMPILER], HASH, null))).toBe(false);
    const none = diagnosticInputs(state([], null, null));
    expect([none.build, none.stale]).toEqual([[], false]);
  });
});

describe('codePanelDiagnostics', () => {
  it('adds the build’s diagnostics only while they belong to the code shown', () => {
    const fresh = diagnosticInputs(state([COMPILER], HASH, HASH));
    expect(codePanelDiagnostics(fresh)).toEqual([...fresh.live, COMPILER]);
    const stale = diagnosticInputs(state([COMPILER], HASH, 'b'.repeat(64)));
    expect(codePanelDiagnostics(stale)).toBe(stale.live);
  });
});
