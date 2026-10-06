/**
 * Nightly E2E security test 2 (docs/spec/08-security.md §8.3 and §8.13, threats T1, T2 and T8 of
 * §8.12): `build_start` and `run_start` called directly through Tauri's internal invoke on a
 * restricted project are refused with `restricted`. The backend decides this itself; nothing of
 * the UI is involved.
 *
 * The project is opened (restricted) through the scripted open dialog, built while restricted
 * (refused, no compiler, no build folder), trusted through the scripted native trust dialog, built
 * and run (the positive control: the same calls work when trusted, and the compiler watch sees the
 * compiler), and then its trust is revoked: running the build that succeeded is refused, and so is
 * building again.
 */
import type { BuildStartResponse, ProjectOpened, TrustResponse } from '@blocks2cpp/ipc-types';
import { describe, expect, it } from 'vitest';

import { launchApp } from '../../support/app';
import { buildFolders, CompilerWatch } from './lib/compilers';
import { describeOutcome, IpcProbe, messageKind, waitForToolchains } from './lib/ipc';
import { copyInto, fixture, profileOf, scratchFolder } from './lib/projects';

/** How long a build may take (the compile timeout). */
const BUILD_TIMEOUT_MS = 120_000;

/** How long the program may take to finish. */
const RUN_TIMEOUT_MS = 30_000;

/** A well-formed build ID that no build has. */
const FORGED_BUILD = 'bd_00000000000000000000000000000000';

describe('E2E security 2: build and run of a restricted project', () => {
  it('refuses build_start and run_start with restricted when called through the internal invoke', async (context) => {
    const folder = scratchFolder(context);
    const file = copyInto(folder, fixture('trust-target.b2c'));
    const app = await launchApp(context, { dialogs: { open: [file], trust: ['trustProject'] } });
    const probe = new IpcProbe(app.driver);
    const profile = profileOf(app);
    await waitForToolchains(probe);

    // Restricted: building and running are refused before anything runs.
    const project = await probe.answer<ProjectOpened>('project_open_dialog');
    expect(project.trust.state).toBe('restricted');
    const build = (config: 'debug' | 'release', channel: string) => ({
      request: { handle: project.handle, document: project.document, config },
      onEvent: { $channel: channel },
    });
    const run = (buildId: string, channel: string) => ({
      request: { buildId, runOptions: { cols: 80, rows: 24 } },
      onOutput: { $channel: `${channel}-output` },
      onEvent: { $channel: `${channel}-events` },
    });
    let watch = CompilerWatch.start(profile);
    expect(
      await probe.check([
        {
          name: 'debug',
          cmd: 'build_start',
          args: build('debug', 'r1'),
          expected: { code: 'restricted' },
        },
        {
          name: 'release',
          cmd: 'build_start',
          args: build('release', 'r2'),
          expected: { code: 'restricted' },
        },
        // No build of this project exists, so there is nothing to run.
        {
          name: 'run',
          cmd: 'run_start',
          args: run(FORGED_BUILD, 'r3'),
          expected: { code: 'unknownBuild' },
        },
      ]),
    ).toEqual([]);
    expect(await watch.stop(), 'compiler processes while restricted').toEqual([]);
    expect(buildFolders(profile), 'build folders while restricted').toEqual([]);
    expect(await probe.channelMessages('r1')).toEqual([]);

    // Trusted through the native dialog (scripted: Trust this project): the same calls work. The
    // watch must see the compiler now, or it could not have seen one before either.
    const granted = await probe.answer<TrustResponse>('trust_grant', {
      request: { handle: project.handle },
    });
    expect(granted.trust).toMatchObject({ state: 'trusted', source: 'project' });
    watch = CompilerWatch.start(profile);
    const { buildId } = await probe.answer<BuildStartResponse>(
      'build_start',
      build('debug', 'trusted-build'),
    );
    const finished = await probe.waitForMessage(
      'trusted-build',
      (message) => messageKind(message) === 'finished',
      BUILD_TIMEOUT_MS,
    );
    expect(finished).toMatchObject({ outcome: 'built' });
    expect((await watch.stop()).length, 'compilers seen during the trusted build').toBeGreaterThan(
      0,
    );
    expect(buildFolders(profile).length).toBeGreaterThan(0);
    await probe.answer('run_start', run(buildId, 'trusted-run'));
    await probe.waitForMessage(
      'trusted-run-events',
      (message) => messageKind(message) === 'exit',
      RUN_TIMEOUT_MS,
    );

    // Revoked: the build that succeeded may not run any more, and nothing builds.
    const revoked = await probe.answer<TrustResponse>('trust_revoke', {
      request: { handle: project.handle },
    });
    expect(revoked.trust.state).toBe('restricted');
    watch = CompilerWatch.start(profile);
    expect(
      await probe.check([
        {
          name: 'run',
          cmd: 'run_start',
          args: run(buildId, 'revoked-run'),
          expected: { code: 'restricted' },
        },
        {
          name: 'debug',
          cmd: 'build_start',
          args: build('debug', 'revoked-build'),
          expected: { code: 'restricted' },
        },
      ]),
    ).toEqual([]);
    expect(await watch.stop(), 'compiler processes after the revocation').toEqual([]);
    expect(await probe.channelMessages('revoked-run-events')).toEqual([]);
    expect(await probe.channelMessages('revoked-build')).toEqual([]);
    const now = await probe.call({
      cmd: 'trust_get',
      args: { request: { handle: project.handle } },
    });
    expect(now.kind === 'answered' ? now.value : describeOutcome(now)).toMatchObject({
      trust: { state: 'restricted' },
    });
  });
});
