/**
 * Nightly E2E security test 6 (docs/spec/08-security.md §8.8 and §8.13; threat T7 of §8.12):
 * navigation away from the app origin and new windows are blocked.
 *
 * Script in the page tries every way it has to leave: assigning `location`, `assign`, `replace`,
 * a link, a link to a new window, `window.open` (also for `about:blank`), a form and a meta
 * refresh, to an external site, the loopback address, a local file and the other platform's
 * spelling of the app origin (a network address here, not the app). After each attempt the page
 * must be the same page (a sentinel global planted before survives), at the app's own address,
 * with one window, and still working (the test hook and the IPC answer).
 */
import { describe, expect, it } from 'vitest';

import { launchApp, waitUntilReady } from '../../support/app';
import { sleep } from '../../support/wait';
import { IpcProbe } from './lib/ipc';
import { pageState, plantSentinel } from './lib/page';

/** The ways the page tries to leave (see {@link ATTEMPT}). */
type Way = 'href' | 'assign' | 'replace' | 'link' | 'blankLink' | 'open' | 'form' | 'meta';

/**
 * Runs in the webview: tries to leave for `arguments[1]` in the way `arguments[0]` names, and
 * returns what `window.open` returned (`'null'` or `'window'`) or `null`. Elements it adds are
 * removed again.
 */
const ATTEMPT = `
  const way = arguments[0];
  const url = arguments[1];
  const add = (element, parent) => { (parent || document.body).appendChild(element); return element; };
  switch (way) {
    case 'href':
      location.href = url;
      return null;
    case 'assign':
      location.assign(url);
      return null;
    case 'replace':
      location.replace(url);
      return null;
    case 'link':
    case 'blankLink': {
      const link = add(document.createElement('a'));
      link.href = url;
      if (way === 'blankLink') {
        link.target = '_blank';
      }
      link.textContent = 'leave';
      link.click();
      link.remove();
      return null;
    }
    case 'open': {
      const opened = window.open(url);
      return opened === null ? 'null' : 'window';
    }
    case 'form': {
      const form = add(document.createElement('form'));
      form.method = 'get';
      form.action = url;
      form.submit();
      form.remove();
      return null;
    }
    case 'meta': {
      const meta = add(document.createElement('meta'), document.head);
      meta.httpEquiv = 'refresh';
      meta.content = '0;url=' + url;
      setTimeout(() => { meta.remove(); }, 3000);
      return null;
    }
    default:
      throw new Error('Unknown way ' + way);
  }
`;

/** The places the page tries to go to on this platform. */
function destinations(): string[] {
  const otherAppOrigin =
    process.platform === 'win32'
      ? ['tauri://localhost/', 'https://tauri.localhost/']
      : ['http://tauri.localhost/'];
  const localFile =
    process.platform === 'win32' ? 'file:///C:/Windows/win.ini' : 'file:///etc/passwd';
  return ['https://example.invalid/', 'http://127.0.0.1:9/', localFile, ...otherAppOrigin];
}

/** How long a navigation may take to happen if it is not blocked. */
const SETTLE_MS = 1_500;

describe('E2E security 6: navigation and new windows', () => {
  it('keeps the window on the app and opens no other window', async (context) => {
    const app = await launchApp(context);
    const { driver } = app;
    const probe = new IpcProbe(driver);
    const start = await pageState(driver);
    const handles = await driver.getAllWindowHandles();
    expect(handles).toHaveLength(1);

    const ways: Way[] = ['href', 'assign', 'replace', 'link', 'blankLink', 'open', 'form', 'meta'];
    const problems: string[] = [];
    let attempt = 0;
    for (const url of [...destinations(), 'about:blank']) {
      for (const way of url === 'about:blank' ? (['open', 'blankLink'] as const) : ways) {
        attempt += 1;
        const sentinel = `attempt-${String(attempt)}`;
        await plantSentinel(driver, sentinel);
        const opened: unknown = await driver.executeScript(ATTEMPT, way, url);
        await sleep(SETTLE_MS);
        const what = `${way} to ${url}`;
        const state = await pageState(driver);
        if (state.sentinel !== sentinel) {
          problems.push(`${what}: the page was replaced (now at ${state.href})`);
          await plantSentinel(driver, 'lost');
        }
        if (state.href !== start.href) {
          problems.push(`${what}: the window went to ${state.href}`);
        }
        if (way === 'open' && opened !== 'null') {
          problems.push(`${what}: window.open returned a window`);
        }
        const now = await driver.getAllWindowHandles();
        if (now.length !== 1) {
          problems.push(`${what}: there are ${String(now.length)} windows`);
          for (const handle of now.filter((entry) => !handles.includes(entry))) {
            await driver.switchTo().window(handle);
            await driver.close();
          }
          await driver.switchTo().window(handles[0] ?? '');
        }
      }
    }
    expect(problems).toEqual([]);

    // The check sees a replaced page: a reload (the app's own address, allowed) loses the
    // sentinel. Then the app is still working: the hook is ready and the backend answers.
    await plantSentinel(driver, 'before-reload');
    await driver.executeScript('location.reload();');
    await waitUntilReady(app);
    const reloaded = await pageState(driver);
    expect(reloaded.sentinel).toBeNull();
    expect(reloaded.href).toBe(start.href);
    expect(await probe.answer('app_info')).toMatchObject({ ipcVersion: 1 });
  });
});
