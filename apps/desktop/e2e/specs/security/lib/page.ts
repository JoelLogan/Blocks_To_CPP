/**
 * What the page looks like to the security tests (docs/spec/08-security.md §8.8): whether script
 * from a project ran (the canary global), whether markup from a project became elements (the
 * marker attribute every payload carries), how many frames there are, and where the window is.
 */
import type { WebDriver } from 'selenium-webdriver';

/** The global every script payload of the fixtures sets; it must stay unset. */
export const CANARY = '__b2cXss';

/** The attribute every markup payload of the fixtures carries; no element may ever have it. */
export const MARKER_ATTRIBUTE = 'data-b2c-xss';

/** The global a test sets to tell whether the page was replaced (by a navigation or a reload). */
export const SENTINEL = '__b2cSecuritySentinel';

/** What {@link pageState} reads. */
export interface PageState {
  /** The canary's value, or `null` while no payload ran. */
  readonly canary: string | null;
  /** How many elements carry {@link MARKER_ATTRIBUTE}. */
  readonly injected: number;
  /** How many frames the page has (the isolation frame is one). */
  readonly frames: number;
  /** `location.href`. */
  readonly href: string;
  /** The sentinel's value, or `null` when the page has none (it was replaced). */
  readonly sentinel: string | null;
}

/** Runs in the webview: the page's state (see {@link PageState}). */
const STATE_SCRIPT = `
  const canary = window[arguments[0]];
  const sentinel = window[arguments[2]];
  return {
    canary: canary === undefined ? null : String(canary),
    injected: document.querySelectorAll('[' + arguments[1] + ']').length,
    frames: document.querySelectorAll('iframe, frame, object, embed').length,
    href: String(location.href),
    sentinel: typeof sentinel === 'string' ? sentinel : null,
  };
`;

/** The page's state now. */
export async function pageState(driver: WebDriver): Promise<PageState> {
  const state: unknown = await driver.executeScript(
    STATE_SCRIPT,
    CANARY,
    MARKER_ATTRIBUTE,
    SENTINEL,
  );
  if (typeof state !== 'object' || state === null) {
    throw new Error('The page returned no state');
  }
  return state as PageState;
}

/** Sets the sentinel to `value` (a later {@link pageState} still has it unless the page changed). */
export async function plantSentinel(driver: WebDriver, value: string): Promise<void> {
  await driver.executeScript('window[arguments[0]] = arguments[1];', SENTINEL, value);
}

/**
 * Problems with a page that must show project text only as text: the canary was set, or an
 * element carries the marker. Empty when the page is clean.
 */
export function injectionProblems(state: PageState): string[] {
  const problems: string[] = [];
  if (state.canary !== null) {
    problems.push(`script from the project ran (${CANARY} = ${JSON.stringify(state.canary)})`);
  }
  if (state.injected > 0) {
    problems.push(
      `${String(state.injected)} element(s) were made from project markup ([${MARKER_ATTRIBUTE}])`,
    );
  }
  return problems;
}

/**
 * Text as the page shows it: runs of white space (non-breaking spaces included, which `\s`
 * matches) as one space, trimmed.
 */
export function shownText(text: string): string {
  return text.replace(/\s+/g, ' ').trim();
}
