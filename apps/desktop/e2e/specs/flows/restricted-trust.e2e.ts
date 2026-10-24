/**
 * Restricted Mode and the trust flow (docs/spec/08-security.md §8.3, 04 §4.10, 07 §7.6.1): a
 * project file copied from elsewhere opens in Restricted Mode, with the banner, Build and Run held
 * back and the C++ still readable; *Trust…* raises the backend's native dialog (answered by the
 * dialog script), *Stay in Restricted Mode* keeps it restricted and *Trust this project* lets it
 * build and run; *Revoke trust* in Settings brings the banner back.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { Key } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import { consoleState, waitForConsoleState } from '../../support/console';
import { clickTestId, pressKey, testIdText } from '../../support/ui';
import { sleep, waitFor } from '../../support/wait';
import { startFlow } from './lib/launch';
import { copyExample, openFromStartPage } from './lib/project';
import {
  accessibleDescription,
  backToEditor,
  BUILD_TIMEOUT_MS,
  heldBack,
  openSettings,
  UI_TIMEOUT_MS,
  waitForTestId,
  waitForToolchain,
} from './lib/ui';

/** The backend accepts one `trust_grant` per project every 2 s (02 §2.5 rate limits). */
const TRUST_GRANT_INTERVAL_MS = 2_000;

/** Waits until the banner's *Trust…* button is idle again (the dialog was answered). */
async function waitForTrustAnswer(driver: Parameters<typeof testIdText>[0]): Promise<void> {
  await waitFor(
    async () => {
      const button = await driver.findElements({ css: '[data-testid="restricted-banner-trust"]' });
      const first = button[0];
      if (first === undefined) {
        return true;
      }
      return (await first.getAttribute('aria-disabled')) !== 'true';
    },
    { timeout: UI_TIMEOUT_MS, message: 'the trust dialog to be answered' },
  );
}

describe('Restricted Mode and trust', () => {
  it('opens a copied example restricted, trusts it through the dialog and revokes the trust', async (context) => {
    const flow = startFlow(context);
    const example = copyExample('hello_world', flow.folders.projects, 'Hello copy.b2c');
    const app = await flow.launch({
      dialogs: { open: [example], trust: ['stayRestricted', 'trustProject'] },
    });
    const { driver, hook } = app;
    await waitForToolchain(app);

    // (1) Open the copy: no trust record matches it, so it is restricted.
    await openFromStartPage(app);
    await waitForTestId(driver, 'restricted-banner', true);
    expect(await testIdText(driver, 'restricted-banner-reason')).toContain(
      'You have not trusted this project on this computer yet',
    );
    expect(await testIdText(driver, 'status-restricted')).not.toBe('');
    expect(await heldBack(driver, 'toolbar-run')).toBe(true);
    expect(await heldBack(driver, 'toolbar-build')).toBe(true);
    // Each says why (04 §4.4).
    expect(await accessibleDescription(driver, 'toolbar-run')).toBe(
      'Restricted Mode: trust this project to run it',
    );
    expect(await accessibleDescription(driver, 'toolbar-build')).toBe(
      'Restricted Mode: trust this project to build it',
    );
    // The generated C++ can still be read (the preview runs in the webview, not the compiler).
    const code = await waitFor(
      async () => {
        const text = await hook.code();
        return text.includes('int main()') ? text : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the C++ of the restricted project' },
    );
    expect(code).toContain('Hello, world!');
    // F5 does not build or run it.
    await pressKey(driver, Key.F5);
    await sleep(1_500);
    expect(await consoleState(driver)).not.toMatch(/Running|Finished/);
    expect(await heldBack(driver, 'toolbar-run')).toBe(true);

    // (2) Trust…, answered "Stay in Restricted Mode": nothing changes.
    await clickTestId(driver, 'restricted-banner-trust');
    await waitForTrustAnswer(driver);
    const answered = Date.now();
    expect(await testIdText(driver, 'restricted-banner-message')).toContain(
      'The project stays in Restricted Mode.',
    );
    expect(await heldBack(driver, 'toolbar-run')).toBe(true);

    // (3) Trust…, answered "Trust this project": the banner goes, Run works.
    await sleep(Math.max(0, answered + TRUST_GRANT_INTERVAL_MS + 250 - Date.now()));
    await clickTestId(driver, 'restricted-banner-trust');
    await waitForTestId(driver, 'restricted-banner', false);
    await waitFor(async () => !(await heldBack(driver, 'toolbar-run')), {
      timeout: UI_TIMEOUT_MS,
      message: 'Run to be enabled once the project is trusted',
    });
    // The record is machine-local (05 §5.8), keyed by the project's path.
    const trustFile = readFileSync(
      path.join(flow.folders.profile, 'machine', 'trust.json'),
      'utf8',
    );
    expect(trustFile).toContain('prj_hello_world');
    await pressKey(driver, Key.F5);
    await waitForConsoleState(driver, /Finished \(exit code 0\)$/, BUILD_TIMEOUT_MS);
    expect(await hook.consoleText()).toContain('Hello, world!');

    // (4) Revoke trust in Settings: restricted again, and the banner is back in the editor.
    await openSettings(driver);
    expect(await testIdText(driver, 'settings-trust-state')).toContain('You trusted this project');
    await clickTestId(driver, 'settings-trust-revoke');
    await waitFor(
      async () => (await testIdText(driver, 'settings-trust-state')).includes('Restricted Mode'),
      { timeout: UI_TIMEOUT_MS, message: 'the project to be restricted after Revoke trust' },
    );
    await backToEditor(driver);
    await waitForTestId(driver, 'restricted-banner', true);
    expect(await heldBack(driver, 'toolbar-run')).toBe(true);
    expect(await heldBack(driver, 'toolbar-build')).toBe(true);
  }, 180_000);
});
