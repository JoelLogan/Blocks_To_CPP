/**
 * The tests' side of `window.__B2C_E2E__` (src/e2e/contract.ts): each method runs the hook's
 * method in the webview through WebDriver's `executeScript`. Elements come back as WebElements.
 */
import type { WebDriver, WebElement } from 'selenium-webdriver';

import {
  E2E_HOOK_NAME,
  type E2eHookContract,
  type E2ePoint,
  type E2eTrustedTypesReport,
} from '../../src/e2e/contract';

/**
 * Runs in the webview: calls a method of the hook. A plain script (not a function from this file),
 * so nothing the test runner adds to compiled functions can end up in the page.
 */
const INVOKE = `
  const hookName = arguments[0];
  const method = arguments[1];
  const args = arguments[2];
  const hook = window[hookName];
  if (typeof hook !== 'object' || hook === null) {
    throw new Error('The end-to-end hook is not installed: is this the e2e build (pnpm build:e2e)?');
  }
  if (typeof hook[method] !== 'function') {
    throw new Error('The end-to-end hook has no method ' + method);
  }
  return hook[method].apply(hook, args);
`;

/** A method of the hook. */
type HookMethod = keyof E2eHookContract;

/** What a hook method's result looks like after WebDriver: elements become WebElements. */
type Wire<T> = [T] extends [Element]
  ? WebElement
  : [T] extends [Element | null]
    ? WebElement | null
    : T;

/** Calls the app's end-to-end hook. */
export class HookClient {
  readonly #driver: WebDriver;

  constructor(driver: WebDriver) {
    this.#driver = driver;
  }

  /** Calls `method` with `args` in the webview and returns its result. */
  async call<K extends HookMethod>(
    method: K,
    ...args: Parameters<E2eHookContract[K]>
  ): Promise<Wire<ReturnType<E2eHookContract[K]>>> {
    return this.#driver.executeScript(INVOKE, E2E_HOOK_NAME, method, args);
  }

  /** Whether the hook is installed and the window has started; false while the page loads. */
  async ready(): Promise<boolean> {
    try {
      return await this.call('ready');
    } catch {
      return false;
    }
  }

  insertBlocks(parentBlockId: string, input: string, blocks: readonly unknown[]): Promise<void> {
    return this.call('insertBlocks', parentBlockId, input, blocks);
  }

  /** The open project's canonical text. */
  documentText(): Promise<string> {
    return this.call('document');
  }

  consoleText(): Promise<string> {
    return this.call('consoleText');
  }

  trustedTypes(): Promise<E2eTrustedTypesReport> {
    return this.call('trustedTypes');
  }

  selectBlock(id: string): Promise<void> {
    return this.call('selectBlock', id);
  }

  code(): Promise<string> {
    return this.call('code');
  }

  blockElement(id: string): Promise<WebElement | null> {
    return this.call('blockElement', id);
  }

  fieldElement(
    blockId: string,
    field: string,
    input: string | null = null,
  ): Promise<WebElement | null> {
    return this.call('fieldElement', blockId, field, input);
  }

  flyoutBlockId(
    type: string,
    fields: Readonly<Record<string, string>> = {},
  ): Promise<string | null> {
    return this.call('flyoutBlockId', type, fields);
  }

  connectionPoint(blockId: string, connection: string): Promise<E2ePoint | null> {
    return this.call('connectionPoint', blockId, connection);
  }

  grabPoint(blockId: string): Promise<E2ePoint | null> {
    return this.call('grabPoint', blockId);
  }

  probeTrustedTypes(): Promise<void> {
    return this.call('probeTrustedTypes');
  }
}
