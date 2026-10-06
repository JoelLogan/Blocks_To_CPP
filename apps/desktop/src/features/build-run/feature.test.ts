/**
 * The build and run feature against a fake backend whose channels the tests play: Build, Run with
 * and without a build, Stop, Run again, the run gate, the acknowledgements, typed input, the exit
 * after its output, and following the open project.
 */
import type { CanonicalResult, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { IpcCallError } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EditorHandle } from '../../app/editor-types';
import type { FeatureContext } from '../../app/features';
import { resetAppStore, useAppStore } from '../../app/store';
import {
  diagnosticFixture,
  documentFixture,
  previewFixture,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from '../../app/testing/fixtures';
import { ACK_INTERVAL_MS } from './acks';
import { GENERATOR_BUG_LABEL } from './buildOutput';
import { ConsoleBridge } from './consoleBridge';
import { createBuildRunFeature } from './feature';
import { RUN_MODE_RESET, RUN_SEPARATOR, STOP_WAIT_MS } from './runController';
import {
  createFakeBackend,
  type FakeBackend,
  FakeConsole,
  type FakeDialogs,
  fakeDialogs,
  featureContext,
  openRunnableProject,
  settle,
} from './testing';

const HASH = 'a'.repeat(64);
const OTHER_HASH = 'b'.repeat(64);

let backend: FakeBackend;
let bridge: ConsoleBridge;
let terminal: FakeConsole;
let ctx: FeatureContext;
let dialogs: FakeDialogs;
let uninstall: () => void;

function install(overrides: Partial<FeatureContext> = {}): void {
  dialogs = fakeDialogs();
  ctx = featureContext(backend.ipc, overrides, dialogs);
  uninstall = createBuildRunFeature({ bridge })(ctx);
}

function state() {
  return useAppStore.getState();
}

function exitEvent(afterSeq: number, message = 'Finished (exit code 0)') {
  return {
    kind: 'exit',
    afterSeq,
    elapsedMs: 1234,
    status: { type: 'exited', code: 0 },
    crash: null,
    sanitizer: null,
    message,
  } as const;
}

const STARTED = {
  kind: 'started',
  containment: 'cgroup',
  mode: 'pty',
  ideHelpers: true,
} as const;

/** Runs `id` and lets everything it starts settle. */
async function run(id: 'run.start' | 'run.again' | 'build.start' | 'run.stop'): Promise<void> {
  void ctx.commands.runCommand(id);
  await settle();
}

/** Finishes build `index` as built, with the project's hash. */
async function finishBuild(index: number, outcome = 'built', hash: string | null = HASH) {
  backend.builds[index]?.send({ kind: 'finished', outcome, projectHash: hash, elapsedMs: 1500 });
  await settle();
}

beforeEach(() => {
  vi.useFakeTimers();
  resetAppStore();
  backend = createFakeBackend();
  bridge = new ConsoleBridge();
  terminal = new FakeConsole();
  bridge.attach(terminal);
  openRunnableProject();
  install();
});

afterEach(() => {
  uninstall();
  vi.useRealTimers();
});

describe('Build', () => {
  it('sends the canonical document and configuration and shows progress, notes and the end', async () => {
    state().actions.setBuild({ config: 'release' });
    await run('build.start');

    expect(backend.builds).toHaveLength(1);
    expect(backend.builds[0]?.request).toEqual({
      handle: projectFixture().handle,
      document: '{"doc":1}',
      config: 'release',
    });
    expect(state().build.status).toBe('building');
    expect(state().build.buildId).toBe(backend.builds[0]?.buildId);
    expect(state().ui.bottomTab).toBe('buildOutput');

    const build = backend.builds[0];
    build?.send({ kind: 'progress', stage: 'generate', done: 1, total: 1 });
    build?.send({ kind: 'progress', stage: 'compile', done: 0, total: 1 });
    expect(state().build.progress).toEqual({ stage: 'compile', done: 0, total: 1 });
    build?.send({
      kind: 'diagnostics',
      items: [
        diagnosticFixture({
          code: 'B2C-T1011',
          severity: 'info',
          message: 'Sanitizers are not available with this compiler; they were left out.',
          source: 'toolchain',
          primary: { part: { kind: 'whole' } },
        }),
        diagnosticFixture({
          code: 'C:-Wunused-variable',
          severity: 'warning',
          message: "g++ warns: unused variable 'x'",
          source: 'compiler',
          raw: "main.cpp:4:9: warning: unused variable 'x' [-Wunused-variable]\n    4 |     int x = 0;\n",
        }),
      ],
    });
    build?.send({ kind: 'progress', stage: 'compile', done: 1, total: 1 });
    build?.send({ kind: 'progress', stage: 'link', done: 1, total: 1 });
    await finishBuild(0);

    const build2 = state().build;
    expect(build2.status).toBe('succeeded');
    expect(build2.progress).toBeNull();
    expect(build2.lastSuccess).toEqual({
      buildId: backend.builds[0]?.buildId,
      projectHash: HASH,
      config: 'release',
    });
    expect(build2.diagnostics.map((diagnostic) => diagnostic.code)).toEqual([
      'B2C-T1011',
      'C:-Wunused-variable',
    ]);
    expect(build2.diagnosticsHash).toBe(HASH);
    expect(build2.output).toEqual([
      { kind: 'progress', text: 'Building Guessing Game (Release)…' },
      { kind: 'progress', text: 'Generating C++ (1/1)' },
      { kind: 'progress', text: 'Compiling (0/1)' },
      {
        kind: 'note',
        text: 'B2C-T1011: Sanitizers are not available with this compiler; they were left out.',
      },
      { kind: 'raw', text: "main.cpp:4:9: warning: unused variable 'x' [-Wunused-variable]" },
      { kind: 'raw', text: '    4 |     int x = 0;' },
      { kind: 'progress', text: 'Compiling (1/1)' },
      { kind: 'progress', text: 'Linking (1/1)' },
      { kind: 'progress', text: 'Built in 1.5 s.' },
    ]);
  });

  it('labels a compiler error in code made from blocks as a bug and shows it in Problems', async () => {
    await run('build.start');
    backend.builds[0]?.send({
      kind: 'diagnostics',
      items: [
        diagnosticFixture({
          code: 'C:error',
          message: "g++ said: 'foo' was not declared",
          source: 'compiler',
          raw: "main.cpp:5:3: error: 'foo' was not declared in this scope",
        }),
      ],
    });
    await finishBuild(0, 'failed');

    const [diagnostic] = state().build.diagnostics;
    expect(diagnostic?.message).toBe(`${GENERATOR_BUG_LABEL}. g++ said: 'foo' was not declared`);
    expect(state().build.output.map((line) => line.text)).toContain(
      `${GENERATOR_BUG_LABEL}: g++ could not compile the C++ made from blocks without errors. Please report it with the project file.`,
    );
    expect(state().build.output.at(-1)?.text).toBe(
      'Build failed after 1.5 s: 1 error (see Problems).',
    );
    expect(state().build.status).toBe('failed');
    expect(state().ui.bottomTab).toBe('problems');
    expect(state().ui.selection).toBe('b007');
  });

  it('does not start the same build twice, but builds changed content again', async () => {
    await run('build.start');
    await run('build.start');
    expect(backend.builds).toHaveLength(1);

    state().actions.updateProject({ canonicalText: '{"doc":2}', contentHash: OTHER_HASH });
    await run('build.start');
    expect(backend.builds).toHaveLength(2);
    expect(backend.builds[1]?.request.document).toBe('{"doc":2}');

    // The backend cancels the first: its end changes nothing any more.
    await finishBuild(0, 'cancelled', null);
    expect(state().build.status).toBe('building');
    await finishBuild(1, 'built', OTHER_HASH);
    expect(state().build.status).toBe('succeeded');
    expect(state().build.lastSuccess?.buildId).toBe(backend.builds[1]?.buildId);
  });

  it('builds what the canvas shows now, through the core, when the editor and core are there', async () => {
    uninstall();
    const canonical = vi.fn((): CanonicalResult => ({
      ok: true,
      text: '{"fresh":true}',
      hash: OTHER_HASH,
      diagnostics: [],
    }));
    const editor = { currentDocument: () => documentFixture() } as unknown as EditorHandle;
    install({
      editor: () => editor,
      core: () => ({ canonical }) as unknown as CoreWasm,
    });
    await run('build.start');
    expect(canonical).toHaveBeenCalledWith(JSON.stringify(documentFixture()));
    expect(backend.builds[0]?.request.document).toBe('{"fresh":true}');
  });

  it('refuses to build or run a canvas the loader refuses, naming the problem', async () => {
    uninstall();
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const tooDeep = diagnosticFixture({
      code: 'B2C-E0104',
      source: 'loader',
      message: 'Lists and objects in the project file are nested more than 128 levels deep.',
      primary: { part: { kind: 'whole' } },
    });
    const canonical = vi.fn((): CanonicalResult => ({ ok: false, diagnostics: [tooDeep] }));
    const editor = { currentDocument: () => documentFixture() } as unknown as EditorHandle;
    install({
      editor: () => editor,
      core: () => ({ canonical }) as unknown as CoreWasm,
    });
    // The store still holds the last version that loaded; it must not be built instead.
    state().actions.updateProject({ canonicalText: '{"doc":3}', contentHash: OTHER_HASH });

    await run('build.start');
    await run('run.start');

    expect(backend.ipc.buildStart).not.toHaveBeenCalled();
    expect(backend.ipc.runStart).not.toHaveBeenCalled();
    expect(dialogs.alert).toHaveBeenCalledTimes(2);
    const [{ title, message }] = dialogs.alert.mock.calls[0] ?? [{ title: '', message: '' }];
    expect(title).toBe('The build could not start');
    expect(message).toContain(
      'B2C-E0104: Lists and objects in the project file are nested more than 128 levels deep.',
    );
  });

  it('puts the last diagnostics back after a cancelled build and keeps warnings when up to date', async () => {
    await run('build.start');
    const warning = diagnosticFixture({
      code: 'C:-Wunused',
      severity: 'warning',
      source: 'compiler',
    });
    backend.builds[0]?.send({ kind: 'diagnostics', items: [warning] });
    await finishBuild(0);

    state().actions.updateProject({ contentHash: OTHER_HASH });
    await run('build.start');
    backend.builds[1]?.send({
      kind: 'diagnostics',
      items: [diagnosticFixture({ code: 'B2C-T1012', source: 'toolchain', severity: 'info' })],
    });
    expect(state().build.diagnostics.map((d) => d.code)).toEqual(['B2C-T1012']);
    await finishBuild(1, 'cancelled', null);
    expect(state().build.diagnostics).toEqual([warning]);
    expect(state().build.diagnosticsHash).toBe(HASH);
    expect(state().build.output.at(-1)?.text).toBe('Build stopped.');

    state().actions.updateProject({ contentHash: HASH });
    state().actions.setBuild({ lastSuccess: null });
    await run('build.start');
    await finishBuild(2, 'upToDate');
    expect(state().build.diagnostics).toEqual([warning]);
    expect(state().build.output.at(-1)?.text).toBe(
      'Up to date: nothing changed since the last build.',
    );
  });

  it('puts back the diagnostics of the last finished build, not those of a replaced one', async () => {
    await run('build.start');
    const finished = diagnosticFixture({
      code: 'C:-Wone',
      severity: 'warning',
      source: 'compiler',
    });
    backend.builds[0]?.send({ kind: 'diagnostics', items: [finished] });
    await finishBuild(0);

    state().actions.updateProject({ contentHash: OTHER_HASH });
    await run('build.start');
    backend.builds[1]?.send({
      kind: 'diagnostics',
      items: [diagnosticFixture({ code: 'C:-Wpartial', severity: 'warning', source: 'compiler' })],
    });
    state().actions.updateProject({ contentHash: 'd'.repeat(64) });
    await run('build.start');
    await finishBuild(1, 'cancelled', null);
    await finishBuild(2, 'cancelled', null);
    expect(state().build.diagnostics).toEqual([finished]);
    expect(state().build.diagnosticsHash).toBe(HASH);
  });

  it('says why a build could not start', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    backend.ipc.buildStart.mockRejectedValueOnce(
      new IpcCallError('build_start', { code: 'tooManySessions' }),
    );
    await run('build.start');
    expect(state().build.status).toBe('failed');
    expect(state().build.output.at(-1)?.text).toBe(
      'Not built: Too many programs are running. Stop one of them, then try again.',
    );
    expect(dialogs.alert).toHaveBeenCalledWith({
      title: 'The build could not start',
      message: 'Too many programs are running. Stop one of them, then try again.',
    });
  });

  it('ignores build events of an unknown shape', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    await run('build.start');
    backend.builds[0]?.send({ kind: 'progress', stage: 'bake', done: 1, total: 1 });
    backend.builds[0]?.send({
      kind: 'finished',
      outcome: 'built',
      projectHash: 'nope',
      elapsedMs: 1,
    });
    await settle();
    expect(state().build.status).toBe('building');
    expect(warn).toHaveBeenCalledTimes(2);
  });

  it('waits for the build ID before it applies a finished event that came first', async () => {
    let answer: (value: { buildId: `bd_${string}` }) => void = () => undefined;
    backend.ipc.buildStart.mockImplementationOnce((_request, onEvent) => {
      onEvent({ kind: 'finished', outcome: 'built', projectHash: HASH, elapsedMs: 3 });
      return new Promise((resolve) => {
        answer = resolve;
      });
    });
    await run('build.start');
    expect(state().build.status).toBe('building');
    answer({ buildId: `bd_${'f'.repeat(32)}` });
    await settle();
    expect(state().build.status).toBe('succeeded');
    expect(state().build.lastSuccess?.buildId).toBe(`bd_${'f'.repeat(32)}`);
  });
});

describe('Run', () => {
  it('builds, runs, writes the output, acknowledges it and shows the exit after it', async () => {
    await run('run.start');
    expect(state().ui.bottomTab).toBe('buildOutput');
    backend.builds[0]?.send({ kind: 'progress', stage: 'compile', done: 0, total: 1 });
    backend.builds[0]?.send({ kind: 'diagnostics', items: [] });
    await finishBuild(0);

    expect(backend.runs).toHaveLength(1);
    const program = backend.runs[0];
    expect(program?.request).toEqual({
      buildId: backend.builds[0]?.buildId,
      runOptions: { cols: 100, rows: 30 },
    });
    expect(state().ui.bottomTab).toBe('console');
    expect(state().run.runId).toBe(program?.runId);

    program?.send(STARTED);
    await settle();
    expect(state().run).toMatchObject({
      status: 'running',
      containment: 'cgroup',
      ideHelpers: true,
      exit: null,
    });
    expect(terminal.focuses).toBe(1);

    terminal.autoResolve = false;
    program?.output('Guess a number from 1 to 100!\r\n');
    program?.output('Your guess: ');
    program?.send(exitEvent(2));
    await settle();
    // The exit waits for both batches to be written.
    expect(state().run.status).toBe('running');
    terminal.resolveWrites(1);
    await settle();
    expect(state().run.status).toBe('running');
    terminal.resolveWrites();
    await settle();
    expect(state().run.status).toBe('exited');
    expect(state().run.exit?.message).toBe('Finished (exit code 0)');
    // Every run starts by resetting the terminal's modes.
    expect(terminal.text).toBe(`${RUN_MODE_RESET}Guess a number from 1 to 100!\r\nYour guess: `);
    expect(backend.ipc.runAck).toHaveBeenLastCalledWith({ runId: program?.runId, seq: 1 });
    await vi.advanceTimersByTimeAsync(ACK_INTERVAL_MS);
    expect(backend.ipc.runAck).toHaveBeenCalledTimes(1);
  });

  it('acknowledges at most every 100 ms, with the highest batch written', async () => {
    await run('run.start');
    await finishBuild(0);
    const program = backend.runs[0];
    program?.send(STARTED);
    await settle();

    program?.output('1');
    await settle();
    expect(backend.ipc.runAck.mock.calls.map(([request]) => request.seq)).toEqual([1]);
    for (let batch = 2; batch <= 6; batch++) {
      program?.output(String(batch));
      await vi.advanceTimersByTimeAsync(16);
    }
    expect(backend.ipc.runAck).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(ACK_INTERVAL_MS);
    expect(backend.ipc.runAck.mock.calls.map(([request]) => request.seq)).toEqual([1, 6]);
  });

  it('runs without building when the last build matches the project', async () => {
    await run('build.start');
    await finishBuild(0);
    await run('run.start');
    expect(backend.builds).toHaveLength(1);
    expect(backend.runs[0]?.request.buildId).toBe(backend.builds[0]?.buildId);
  });

  it('builds again when the content, the configuration or the compiler changed', async () => {
    await run('build.start');
    await finishBuild(0);

    state().actions.updateProject({ contentHash: OTHER_HASH });
    await run('run.start');
    expect(backend.builds).toHaveLength(2);
    await finishBuild(1, 'built', OTHER_HASH);
    expect(backend.runs[0]?.request.buildId).toBe(backend.builds[1]?.buildId);
    backend.runs[0]?.send(exitEvent(0));
    await settle();

    state().actions.setBuild({ config: 'release' });
    await run('run.start');
    expect(backend.builds).toHaveLength(3);
    await finishBuild(2, 'built', OTHER_HASH);
    backend.runs[1]?.send(exitEvent(0));
    await settle();

    state().actions.setToolchains({
      list: [
        toolchainFixture({ selected: false }),
        toolchainFixture({ id: 'tc_fedcba9876543210', selected: true }),
      ],
    });
    await run('run.start');
    expect(backend.builds).toHaveLength(4);
  });

  it('stops a running program first, shows Stopped, then runs again', async () => {
    await run('run.start');
    await finishBuild(0);
    const first = backend.runs[0];
    first?.send(STARTED);
    first?.output('working…');
    await settle();

    await run('run.start');
    expect(backend.ipc.runStop).toHaveBeenCalledWith({ runId: first?.runId });
    expect(backend.runs).toHaveLength(1);
    first?.send({
      ...exitEvent(1, 'Stopped'),
      status: { type: 'stopped' },
    });
    await settle();
    expect(backend.runs).toHaveLength(2);
    expect(state().run.status).toBe('starting');
    expect(state().run.exit?.message).toBe('Stopped');

    const second = backend.runs[1];
    second?.send(STARTED);
    second?.output('again');
    await settle();
    expect(state().run.status).toBe('running');
    expect(terminal.text).toBe(`${RUN_MODE_RESET}working…${RUN_SEPARATOR}again`);
  });

  it("resets the terminal's modes after Clear too, without a separator", async () => {
    await run('run.start');
    await finishBuild(0);
    backend.runs[0]?.send(STARTED);
    // The program hides the cursor and ends.
    backend.runs[0]?.output('\u001b[?25lbye');
    backend.runs[0]?.send(exitEvent(1));
    await settle();
    bridge.clear();
    terminal.text = '';

    await run('run.again');
    backend.runs[1]?.send(STARTED);
    backend.runs[1]?.output('hello');
    await settle();
    expect(terminal.text).toBe(`${RUN_MODE_RESET}hello`);
  });

  it('starts the new run anyway when the old one does not report its end', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    await run('run.start');
    await finishBuild(0);
    backend.runs[0]?.send(STARTED);
    await settle();

    void ctx.commands.runCommand('run.again');
    await vi.advanceTimersByTimeAsync(STOP_WAIT_MS - 1);
    expect(backend.runs).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    await settle();
    expect(backend.runs).toHaveLength(2);
    // The old run's late output no longer reaches the console.
    backend.runs[0]?.output('late');
    await settle();
    expect(terminal.text).not.toContain('late');
  });

  it('builds again and retries once when the backend says the build is stale', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    await run('build.start');
    await finishBuild(0);
    backend.ipc.runStart.mockRejectedValueOnce(
      new IpcCallError('run_start', { code: 'staleBuild' }),
    );
    await run('run.start');
    expect(backend.builds).toHaveLength(2);
    await finishBuild(1);
    expect(backend.ipc.runStart).toHaveBeenCalledTimes(2);
    expect(backend.runs).toHaveLength(1);
    expect(backend.runs[0]?.request.buildId).toBe(backend.builds[1]?.buildId);
  });

  it('says why the program could not start, and restores the header', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    backend.ipc.runStart.mockRejectedValueOnce(
      new IpcCallError('run_start', { code: 'tooManySessions' }),
    );
    await run('run.start');
    await finishBuild(0);
    expect(dialogs.alert).toHaveBeenCalledWith({
      title: 'The program could not start',
      message: 'Too many programs are running. Stop one of them, then try again.',
    });
    expect(state().run.status).toBe('idle');
  });

  it('says why the build of a run could not start', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    backend.ipc.buildStart.mockRejectedValueOnce(
      new IpcCallError('build_start', { code: 'payloadTooLarge', limit: 33_554_432 }),
    );
    await run('run.start');
    expect(dialogs.alert).toHaveBeenCalledWith({
      title: 'The build could not start',
      message: 'The project is too large to build.',
    });
    expect(backend.ipc.runStart).not.toHaveBeenCalled();
  });

  it('does not run after a failed build, and shows the first error', async () => {
    await run('run.start');
    backend.builds[0]?.send({
      kind: 'diagnostics',
      items: [
        diagnosticFixture({ code: 'C:link', source: 'linker', message: 'undefined reference' }),
      ],
    });
    await finishBuild(0, 'failed');
    expect(backend.ipc.runStart).not.toHaveBeenCalled();
    expect(state().ui.bottomTab).toBe('problems');
  });

  it('follows a build that a newer one replaced', async () => {
    await run('run.start');
    // Build with the same content replaces nothing; a toolchain change does.
    state().actions.setToolchains({
      list: [toolchainFixture({ id: 'tc_fedcba9876543210' })],
    });
    await run('build.start');
    expect(backend.builds).toHaveLength(2);
    await finishBuild(0, 'cancelled', null);
    expect(backend.ipc.runStart).not.toHaveBeenCalled();
    await finishBuild(1);
    expect(backend.runs[0]?.request.buildId).toBe(backend.builds[1]?.buildId);
  });

  it('writes the skipped marker where the output was dropped', async () => {
    await run('run.start');
    await finishBuild(0);
    const program = backend.runs[0];
    program?.send(STARTED);
    terminal.autoResolve = false;
    program?.output('a');
    program?.send({ kind: 'skipped', lines: 1_204_331, afterSeq: 1 });
    program?.output('z');
    await settle();
    // Queued right after batch 1, before the kept tail, although batch 1 is not written yet.
    expect(terminal.skipped).toEqual([1_204_331]);
    expect(terminal.text).toBe(`${RUN_MODE_RESET}a[skipped 1204331]z`);
    terminal.resolveWrites();
    await settle();
    expect(terminal.text).toBe(`${RUN_MODE_RESET}a[skipped 1204331]z`);
  });

  it('writes a skipped marker that arrives before its batch right after that batch', async () => {
    await run('run.start');
    await finishBuild(0);
    const program = backend.runs[0];
    program?.send(STARTED);
    terminal.autoResolve = false;
    program?.output('a');
    // The event channel overtook batch 2: the marker waits for it, then follows it.
    program?.send({ kind: 'skipped', lines: 7, afterSeq: 2 });
    await settle();
    expect(terminal.skipped).toEqual([]);
    program?.output('b');
    program?.output('z');
    await settle();
    expect(terminal.text).toBe(`${RUN_MODE_RESET}ab[skipped 7]z`);
    // The exit still waits until all the output is on screen.
    program?.send(exitEvent(3));
    await settle();
    expect(state().run.status).toBe('running');
    terminal.resolveWrites();
    await settle();
    expect(state().run.status).toBe('exited');
  });

  it('shows the exit anyway when output batches never arrive', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    await run('run.start');
    await finishBuild(0);
    const program = backend.runs[0];
    program?.send(STARTED);
    program?.output('only one');
    program?.send(exitEvent(3));
    await settle();
    expect(state().run.status).toBe('running');
    await vi.advanceTimersByTimeAsync(2000);
    expect(state().run.status).toBe('exited');
    expect(warn).toHaveBeenCalled();
  });

  it('sends typed text as base64 in chunks of at most 64 KiB, and resizes', async () => {
    await run('run.start');
    await finishBuild(0);
    const program = backend.runs[0];
    program?.send(STARTED);
    await settle();

    terminal.type('42\r');
    await settle();
    // '42\r' is 0x34 0x32 0x0D.
    expect(backend.ipc.runInput).toHaveBeenCalledWith({ runId: program?.runId, data: 'NDIN' });

    terminal.type('x'.repeat(70_000));
    await settle();
    const sizes = backend.ipc.runInput.mock.calls
      .slice(1)
      .map(([request]) => atob(request.data).length);
    expect(sizes).toEqual([65_536, 70_000 - 65_536]);
    expect(backend.inputs().join('')).toBe(`42\r${'x'.repeat(70_000)}`);

    terminal.resizeTo({ cols: 120, rows: 40 });
    await settle();
    expect(backend.ipc.runResize).toHaveBeenCalledWith({
      runId: program?.runId,
      cols: 120,
      rows: 40,
    });

    program?.send(exitEvent(0));
    await settle();
    terminal.type('ignored');
    await settle();
    expect(backend.inputs().join('')).not.toContain('ignored');
  });
});

describe('Stop', () => {
  it('cancels the running build', async () => {
    await run('run.start');
    await run('run.stop');
    expect(backend.ipc.buildCancel).toHaveBeenCalledWith({ buildId: backend.builds[0]?.buildId });
    await finishBuild(0, 'cancelled', null);
    expect(backend.ipc.runStart).not.toHaveBeenCalled();
    expect(state().build.status).toBe('cancelled');
  });

  it('cancels a build whose ID has not arrived yet as soon as it does', async () => {
    let answer: (value: { buildId: `bd_${string}` }) => void = () => undefined;
    backend.ipc.buildStart.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    await run('build.start');
    await run('run.stop');
    expect(backend.ipc.buildCancel).not.toHaveBeenCalled();
    answer({ buildId: `bd_${'1'.repeat(32)}` });
    await settle();
    expect(backend.ipc.buildCancel).toHaveBeenCalledWith({ buildId: `bd_${'1'.repeat(32)}` });
  });

  it('stops the running program', async () => {
    await run('run.start');
    await finishBuild(0);
    backend.runs[0]?.send(STARTED);
    await settle();
    await run('run.stop');
    expect(backend.ipc.runStop).toHaveBeenCalledWith({ runId: backend.runs[0]?.runId });
  });

  it('stops a program whose ID arrives after Stop', async () => {
    let answer: (value: { runId: `rn_${string}` }) => void = () => undefined;
    backend.ipc.runStart.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    await run('run.start');
    await finishBuild(0);
    await run('run.stop');
    answer({ runId: `rn_${'2'.repeat(32)}` });
    await settle();
    expect(backend.ipc.runStop).toHaveBeenCalledWith({ runId: `rn_${'2'.repeat(32)}` });
  });
});

describe('the run gate', () => {
  it('does nothing without a project', async () => {
    state().actions.setProject(null);
    await run('run.start');
    await run('build.start');
    expect(backend.ipc.buildStart).not.toHaveBeenCalled();
  });

  it('refuses in Restricted Mode and says why', async () => {
    state().actions.updateProject({
      trust: {
        state: 'restricted',
        source: null,
        restrictedReason: 'noRecord',
        markOfTheWeb: false,
      },
    });
    await run('run.again');
    expect(backend.ipc.buildStart).not.toHaveBeenCalled();
    expect(dialogs.alert).toHaveBeenCalledWith({
      title: 'Restricted Mode',
      message: 'Restricted Mode: trust this project to run it',
    });
  });

  it('opens the toolchain setup page without a usable compiler', async () => {
    state().actions.setToolchains({ list: [toolchainFixture({ usable: false })] });
    await run('run.start');
    expect(dialogs.alert).toHaveBeenCalledWith({
      title: 'No C++ compiler',
      message: 'Blocks2Cpp found no C++ compiler (g++) it can use.',
    });

    const unregister = ctx.screens.registerScreen('toolchainSetup', () => null);
    await run('build.start');
    expect(state().ui.screen).toBe('toolchainSetup');
    expect(backend.ipc.buildStart).not.toHaveBeenCalled();
    unregister();
  });

  it.each(['disableRun', 'showProblems'] as const)(
    'shows the first error instead of running with errors (%s)',
    async (onErrors) => {
      state().actions.setSettings({ value: settingsFixture({ run: { onErrors } }) });
      state().actions.setAnalysis({ preview: previewFixture([diagnosticFixture()]) });
      state().actions.setUi({ bottomTab: 'console', bottomCollapsed: true });
      const selectBlock = vi.fn();
      uninstall();
      install({ editor: () => ({ selectBlock }) as unknown as EditorHandle });

      await run('run.start');
      expect(backend.ipc.buildStart).not.toHaveBeenCalled();
      expect(state().ui).toMatchObject({
        bottomTab: 'problems',
        bottomCollapsed: false,
        selection: 'b007',
        screen: 'editor',
      });
      expect(selectBlock).toHaveBeenCalledWith('b007', { center: true });
    },
  );
});

describe('the open project', () => {
  it('stops following the old project when another one opens', async () => {
    await run('run.start');
    await finishBuild(0);
    backend.runs[0]?.send(STARTED);
    await settle();

    state().actions.setProject(
      projectFixture({ handle: `ph_${'9'.repeat(32)}`, document: documentFixture('Other') }),
    );
    // Reset, not cleared: a clear would still write the old program's queued output.
    expect(terminal.resets).toBe(1);
    expect(terminal.clears).toBe(0);
    backend.runs[0]?.output('old output');
    backend.runs[0]?.send(exitEvent(1));
    await settle();
    expect(terminal.text).toBe('');
    expect(state().run.status).toBe('idle');
  });

  it("starts the new project's first run without a separator, with the modes reset", async () => {
    await run('run.start');
    await finishBuild(0);
    backend.runs[0]?.send(STARTED);
    backend.runs[0]?.output('old output');
    await settle();

    state().actions.setProject(
      projectFixture({ handle: `ph_${'9'.repeat(32)}`, document: documentFixture('Other') }),
    );
    await run('run.start');
    await finishBuild(1);
    backend.runs[1]?.send(STARTED);
    backend.runs[1]?.output('new output');
    await settle();
    expect(terminal.text).toBe(`${RUN_MODE_RESET}new output`);
  });

  it('forgets a build of a closed project', async () => {
    await run('run.start');
    state().actions.setProject(null);
    await finishBuild(0);
    expect(backend.ipc.runStart).not.toHaveBeenCalled();
    expect(state().build.status).toBe('idle');
  });
});

describe('uninstalling', () => {
  it('removes the commands and the console connection', async () => {
    uninstall();
    expect(ctx.commands.hasCommand('run.start')).toBe(false);
    expect(ctx.commands.hasCommand('build.start')).toBe(false);
    terminal.type('nothing');
    await settle();
    expect(backend.ipc.runInput).not.toHaveBeenCalled();
    uninstall = () => undefined;
  });
});
