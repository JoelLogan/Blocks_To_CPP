import { beforeEach, describe, expect, it } from 'vitest';

import {
  diagnosticFixture,
  previewFixture,
  projectFixture,
  toolchainFixture,
} from '../testing/fixtures';
import {
  MAX_BUILD_OUTPUT_LINE_LENGTH,
  MAX_BUILD_OUTPUT_LINES,
  resetAppStore,
  useAppStore,
} from '.';
import {
  buildOnlyDiagnostics,
  firstError,
  hasUsableToolchain,
  liveErrors,
  problemCount,
  selectedToolchain,
} from './selectors';

const actions = () => useAppStore.getState().actions;

beforeEach(() => {
  resetAppStore();
});

describe('the app store', () => {
  it('starts with no project, on the start screen, in Debug', () => {
    const state = useAppStore.getState();
    expect(state.project).toBeNull();
    expect(state.appInfo).toBeNull();
    expect(state.ui.screen).toBe('start');
    expect(state.build).toMatchObject({ status: 'idle', config: 'debug', output: [] });
    expect(state.run.status).toBe('idle');
  });

  it('merges patches into each slice', () => {
    actions().setUi({ bottomTab: 'problems' });
    actions().setBuild({ status: 'building' });
    actions().setRun({ status: 'running', ideHelpers: true });
    actions().setToolchains({ discovering: true });
    actions().setSettings({
      notices: [{ key: 'console.scrollbackLines', reason: 'invalidValue' }],
    });
    actions().setAnalysis({ seq: 3, notice: 'syncFailed' });
    actions().setAppInfo({
      appVersion: '0.1.0',
      ipcVersion: 1,
      platform: 'windows',
      catalogVersion: '1.0.0',
    });

    const state = useAppStore.getState();
    expect(state.ui).toMatchObject({ bottomTab: 'problems', screen: 'start' });
    expect(state.build).toMatchObject({ status: 'building', config: 'debug' });
    expect(state.run).toMatchObject({ status: 'running', ideHelpers: true, runId: null });
    expect(state.toolchains.discovering).toBe(true);
    expect(state.settings.notices).toHaveLength(1);
    expect(state.analysis).toMatchObject({ seq: 3, notice: 'syncFailed', preview: null });
    expect(state.appInfo?.platform).toBe('windows');
  });

  it('updates the open project, and ignores updates without one', () => {
    actions().updateProject({ dirty: true });
    expect(useAppStore.getState().project).toBeNull();

    actions().setProject(projectFixture());
    actions().updateProject({ dirty: true });
    expect(useAppStore.getState().project?.dirty).toBe(true);
  });

  it("forgets the previous project's session when another project opens", () => {
    actions().setProject(projectFixture());
    actions().setBuild({ config: 'release', status: 'failed', diagnostics: [diagnosticFixture()] });
    actions().setRun({ status: 'exited' });
    actions().setAnalysis({ preview: previewFixture([]) });
    actions().setUi({ selection: 'b007', hoverBlock: 'b008', bottomTab: 'problems' });

    actions().setProject(projectFixture({ handle: 'ph_ffffffffffffffffffffffffffffffff' }));

    const state = useAppStore.getState();
    expect(state.build).toMatchObject({ status: 'idle', config: 'release', diagnostics: [] });
    expect(state.run.status).toBe('idle');
    expect(state.analysis.preview).toBeNull();
    expect(state.ui).toMatchObject({ selection: null, hoverBlock: null, bottomTab: 'problems' });
  });

  it('keeps the session when the same project is set again', () => {
    actions().setProject(projectFixture());
    actions().setBuild({ status: 'succeeded' });
    actions().setProject(projectFixture({ dirty: true }));

    expect(useAppStore.getState().build.status).toBe('succeeded');
    expect(useAppStore.getState().project?.dirty).toBe(true);
  });

  it('closes the project and its session with null', () => {
    actions().setProject(projectFixture());
    actions().setRun({ status: 'running' });
    actions().setProject(null);

    expect(useAppStore.getState().project).toBeNull();
    expect(useAppStore.getState().run.status).toBe('idle');
  });

  it('bounds the build output', () => {
    actions().appendBuildOutput([]);
    expect(useAppStore.getState().build.output).toEqual([]);

    const long = 'x'.repeat(MAX_BUILD_OUTPUT_LINE_LENGTH + 10);
    actions().appendBuildOutput([{ kind: 'raw', text: long }]);
    const [line] = useAppStore.getState().build.output;
    expect(line?.text).toHaveLength(MAX_BUILD_OUTPUT_LINE_LENGTH);
    expect(line?.text.endsWith('…')).toBe(true);

    const many = Array.from({ length: MAX_BUILD_OUTPUT_LINES + 5 }, (_, index) => ({
      kind: 'progress' as const,
      text: String(index),
    }));
    actions().appendBuildOutput(many);
    const { output } = useAppStore.getState().build;
    expect(output).toHaveLength(MAX_BUILD_OUTPUT_LINES);
    expect(output[0]?.text).toBe('5');
    expect(output.at(-1)?.text).toBe(String(MAX_BUILD_OUTPUT_LINES + 4));

    actions().appendBuildOutput([{ kind: 'note', text: 'last' }]);
    expect(useAppStore.getState().build.output).toHaveLength(MAX_BUILD_OUTPUT_LINES);
    expect(useAppStore.getState().build.output.at(-1)).toEqual({ kind: 'note', text: 'last' });
  });
});

describe('selectors', () => {
  it('picks the selected usable toolchain, else the first usable one', () => {
    const first = toolchainFixture({ id: 'tc_1111111111111111', selected: false });
    const second = toolchainFixture({ id: 'tc_2222222222222222', selected: true });
    const broken = toolchainFixture({ id: 'tc_3333333333333333', selected: true, usable: false });
    const state = (list: ReturnType<typeof toolchainFixture>[]) => ({
      toolchains: { list, discovering: false, setupInfo: null },
    });

    expect(selectedToolchain(state([first, second]))?.id).toBe(second.id);
    expect(selectedToolchain(state([broken, first]))?.id).toBe(first.id);
    expect(selectedToolchain(state([broken]))).toBeNull();
    expect(hasUsableToolchain(state([broken]))).toBe(false);
    expect(hasUsableToolchain(state([broken, first]))).toBe(true);
  });

  it('counts live problems plus the ones only a build finds', () => {
    actions().setAnalysis({
      preview: previewFixture([diagnosticFixture(), diagnosticFixture({ severity: 'warning' })]),
    });
    actions().setBuild({
      diagnostics: [
        diagnosticFixture({ source: 'analyser' }),
        diagnosticFixture({ source: 'compiler', code: 'C:E1001' }),
        diagnosticFixture({ source: 'linker', code: 'C:L1001', severity: 'warning' }),
      ],
    });
    const state = useAppStore.getState();

    expect(liveErrors(state)).toHaveLength(1);
    expect(buildOnlyDiagnostics(state).map((d) => d.source)).toEqual(['compiler', 'linker']);
    expect(problemCount(state)).toBe(4);
  });

  it('finds the first error: live first, then a build one', () => {
    expect(firstError(useAppStore.getState())).toBeNull();

    const compiler = diagnosticFixture({ source: 'compiler', code: 'C:E1001' });
    actions().setBuild({ diagnostics: [diagnosticFixture({ severity: 'warning' }), compiler] });
    expect(firstError(useAppStore.getState())).toBe(compiler);

    const live = diagnosticFixture({ code: 'B2C-E0301' });
    actions().setAnalysis({ preview: previewFixture([live]) });
    expect(firstError(useAppStore.getState())).toStrictEqual(live);
  });
});
