/**
 * Nightly E2E security test 7 (docs/spec/08-security.md §8.3, §8.3.1 and §8.13; threats T1 and
 * T2 of §8.12): a trust-relevant change made outside the app returns the project to Restricted
 * Mode.
 *
 * The trust record holds the security hash of the project's Raw C++, libraries, packs and
 * defines; trust is evaluated again, from `trust.json` on disk, at every open and reload. So:
 *
 * - a define changed or added in the project file by another program makes the project
 *   *changed outside* at the next reload (an edit that is not trust-relevant does not);
 * - an edit of `trust.json` itself (another hash, the record removed, the file broken) makes it
 *   restricted at the next reload too;
 * - in the editor, the external-change dialog's *Reload* brings the Restricted Mode banner back.
 *
 * Every time, building is refused again with `restricted`.
 */
import { writeFileSync } from 'node:fs';

import type { ProjectOpened, ProjectReloadResponse, TrustResponse } from '@blocks2cpp/ipc-types';
import { By } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { launchApp } from '../../support/app';
import { runHeldBack } from '../../support/editor';
import { clickTestId, testId, testIdText, textContent } from '../../support/ui';
import { sleep, waitFor } from '../../support/wait';
import { buildFolders } from './lib/compilers';
import { IpcProbe, waitForToolchains } from './lib/ipc';
import {
  copyInto,
  editJson,
  fixture,
  objectAt,
  profileOf,
  readJson,
  scratchFolder,
  trustStore,
} from './lib/projects';

/** One trust_grant per project every 2 s (08 §8.8): the wait between two grants. */
const GRANT_INTERVAL_MS = 2_100;

/** Commands on one project handle, through the internal invoke. */
class Project {
  readonly #probe: IpcProbe;
  readonly handle: string;
  document: string;

  private constructor(probe: IpcProbe, handle: string, document: string) {
    this.#probe = probe;
    this.handle = handle;
    this.document = document;
  }

  /** Opens the next file of the dialog script; it must open restricted (no record yet). */
  static async open(probe: IpcProbe): Promise<Project> {
    const opened = await probe.answer<ProjectOpened>('project_open_dialog');
    expect(opened.trust).toMatchObject({ state: 'restricted', restrictedReason: 'noRecord' });
    return new Project(probe, opened.handle, opened.document);
  }

  /** Trusts it through the native dialog (scripted: Trust this project). */
  async grant(): Promise<void> {
    const { trust } = await this.#probe.answer<TrustResponse>('trust_grant', {
      request: { handle: this.handle },
    });
    expect(trust).toMatchObject({ state: 'trusted', source: 'project' });
  }

  /** Reads the file again (trust is evaluated again) and returns the new trust. */
  async reload(): Promise<ProjectReloadResponse['trust']> {
    const reloaded = await this.#probe.answer<ProjectReloadResponse>('project_reload', {
      request: { handle: this.handle },
    });
    this.document = reloaded.document;
    return reloaded.trust;
  }

  /** Fails unless building the current document is refused with `restricted`. */
  async expectBuildRefused(): Promise<void> {
    expect(
      await this.#probe.check([
        {
          name: 'build',
          cmd: 'build_start',
          args: {
            request: { handle: this.handle, document: this.document, config: 'debug' },
            onEvent: { $channel: `refused-${this.handle}` },
          },
          expected: { code: 'restricted' },
        },
      ]),
    ).toEqual([]);
  }
}

/** The first define of a project file (the fixture has `GAME_LEVEL`). */
function firstDefine(project: Record<string, unknown>): Record<string, unknown> {
  const defines = objectAt(project, 'project', 'build')['defines'];
  const first: unknown = Array.isArray(defines) ? defines[0] : undefined;
  if (typeof first !== 'object' || first === null) {
    throw new Error('The project has no define');
  }
  return first as Record<string, unknown>;
}

/** The first top-level block of the first module of a project file. */
function mainBlock(project: Record<string, unknown>): Record<string, unknown> {
  const modules = project['modules'];
  const first: unknown = Array.isArray(modules) ? modules[0] : undefined;
  if (typeof first !== 'object' || first === null) {
    throw new Error('The project has no module');
  }
  const blocks = objectAt(first as Record<string, unknown>, 'workspace')['blocks'];
  const block: unknown = Array.isArray(blocks) ? blocks[0] : undefined;
  if (typeof block !== 'object' || block === null) {
    throw new Error('The module has no block');
  }
  return block as Record<string, unknown>;
}

/** The project records of a trust store file. */
function projectRecords(store: Record<string, unknown>): Record<string, unknown>[] {
  const records = store['projects'];
  if (!Array.isArray(records)) {
    throw new Error('trust.json has no project records');
  }
  return records as Record<string, unknown>[];
}

describe('E2E security 7: trust-relevant changes outside the app', () => {
  it('returns the project to Restricted Mode when a define changes in the file', async (context) => {
    const folder = scratchFolder(context);
    const file = copyInto(folder, fixture('trust-target.b2c'));
    const app = await launchApp(context, {
      dialogs: { open: [file], trust: ['trustProject', 'trustProject'] },
    });
    const probe = new IpcProbe(app.driver);
    const project = await Project.open(probe);
    await project.grant();

    // Not trust-relevant: another name and another position keep the trust.
    editJson(file, (value) => {
      objectAt(value, 'project')['name'] = 'Renamed outside';
      mainBlock(value)['x'] = 400;
    });
    expect(await project.reload()).toMatchObject({ state: 'trusted', source: 'project' });

    // A define's value changed outside: changed outside, and building is refused.
    editJson(file, (value) => {
      firstDefine(value)['value'] = { int: 2 };
    });
    expect(await project.reload()).toMatchObject({
      state: 'restricted',
      restrictedReason: 'changedOutside',
    });
    await project.expectBuildRefused();

    // Trusted again as it is now, then a define added outside: the same.
    await sleep(GRANT_INTERVAL_MS);
    await project.grant();
    editJson(file, (value) => {
      const build = objectAt(value, 'project', 'build');
      const defines = build['defines'];
      build['defines'] = [
        ...(Array.isArray(defines) ? (defines as unknown[]) : []),
        { name: 'GAME_CHEATS', value: { bool: true } },
      ];
    });
    expect(await project.reload()).toMatchObject({
      state: 'restricted',
      restrictedReason: 'changedOutside',
    });
    await project.expectBuildRefused();
    expect(buildFolders(profileOf(app))).toEqual([]);
  });

  it('returns the project to Restricted Mode when trust.json is edited outside the app', async (context) => {
    const folder = scratchFolder(context);
    const file = copyInto(folder, fixture('trust-target.b2c'));
    const app = await launchApp(context, {
      dialogs: { open: [file], trust: ['trustProject', 'trustProject', 'trustProject'] },
    });
    const probe = new IpcProbe(app.driver);
    const store = trustStore(profileOf(app));
    const project = await Project.open(probe);

    // Another hash in the record: changed outside.
    await project.grant();
    editJson(store, (value) => {
      const records = projectRecords(value);
      expect(records).toHaveLength(1);
      for (const record of records) {
        record['rawCodeHashAtGrant'] = '0'.repeat(64);
      }
    });
    expect(await project.reload()).toMatchObject({
      state: 'restricted',
      restrictedReason: 'changedOutside',
    });
    await project.expectBuildRefused();

    // The record removed: no record.
    await sleep(GRANT_INTERVAL_MS);
    await project.grant();
    editJson(store, (value) => {
      expect(projectRecords(value)).toHaveLength(1);
      value['projects'] = [];
    });
    expect(await project.reload()).toMatchObject({
      state: 'restricted',
      restrictedReason: 'noRecord',
    });
    await project.expectBuildRefused();

    // The file broken: nothing is trusted.
    await sleep(GRANT_INTERVAL_MS);
    await project.grant();
    expect(projectRecords(readJson(store) as Record<string, unknown>)).toHaveLength(1);
    writeFileSync(store, '{"format": "blocks2cpp/trust", "formatVersion": 1, "projects": [');
    expect(await project.reload()).toMatchObject({ state: 'restricted' });
    await project.expectBuildRefused();
    expect(buildFolders(profileOf(app))).toEqual([]);
  });

  it('shows Restricted Mode again after Reload when a define changed outside the open project', async (context) => {
    const folder = scratchFolder(context);
    const file = copyInto(folder, fixture('trust-target.b2c'));
    const app = await launchApp(context, { dialogs: { open: [file], trust: ['trustProject'] } });
    const { driver } = app;
    await waitForToolchains(new IpcProbe(driver));

    // Opened in the editor, then trusted through the banner (scripted: Trust this project).
    await clickTestId(driver, 'start-open');
    await testId(driver, 'restricted-banner', 20_000);
    await clickTestId(driver, 'restricted-banner-trust');
    await waitFor(
      async () =>
        (await driver.findElements(By.css('[data-testid="restricted-banner"]'))).length === 0,
      { timeout: 15_000, message: 'the project to be trusted' },
    );
    await waitFor(async () => !(await runHeldBack(driver)), {
      timeout: 15_000,
      message: 'Run to be offered',
    });

    // Another program changes the define; the editor asks, and Reload is chosen.
    editJson(file, (value) => {
      firstDefine(value)['value'] = { int: 99 };
    });
    const reload = await waitFor(
      async () => {
        for (const button of await driver.findElements(
          By.css('[data-testid="app-dialog"] button'),
        )) {
          if ((await textContent(driver, button)) === 'Reload') {
            return button;
          }
        }
        return null;
      },
      { timeout: 20_000, message: 'the external-change dialog with Reload' },
    );
    await reload.click();

    // Restricted Mode again, because the define changed outside, and Run is held back.
    await testId(driver, 'restricted-banner', 15_000);
    expect(await testIdText(driver, 'restricted-banner-reason')).toContain(
      'changed outside Blocks2Cpp',
    );
    expect(await runHeldBack(driver)).toBe(true);
  });
});
