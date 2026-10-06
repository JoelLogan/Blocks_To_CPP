/**
 * The Trusted Types trial's collector (docs/spec/08-security.md §8.8). The backend serves the app's
 * HTML with `Content-Security-Policy-Report-Only: require-trusted-types-for 'script'`, so the
 * webview reports every string that reaches an HTML or script sink without a Trusted Types policy,
 * and the enforced Content Security Policy reports what it blocks. This module counts those
 * `securitypolicyviolation` events.
 *
 * It keeps only how many violations there were and which directives they were against; never the
 * sample (`sample` holds the start of the offending text, which can be project content), the
 * blocked URI or the source location. In end-to-end builds the counts are exposed to the test
 * harness (src/e2e/), and the Windows E2E job writes them to its summary.
 */

/** What the collector has seen since it started. */
export interface TrustedTypesReport {
  /** How many violation events there were (report-only and enforced together). */
  readonly count: number;
  /** The directives that were violated, each once, in alphabetical order. */
  readonly directives: readonly string[];
}

/** The most distinct directives the collector remembers; later new ones are only counted. */
export const MAX_DIRECTIVES = 32;

/**
 * What a directive name may look like (`require-trusted-types-for`, `script-src-elem`). Anything
 * else is recorded as {@link OTHER_DIRECTIVE}, so no event text is ever kept.
 */
const DIRECTIVE_NAME = /^[a-z][a-z-]{0,63}$/;

/** The name recorded for a directive that does not look like one. */
export const OTHER_DIRECTIVE = 'other';

/** The part of a violation event the collector reads. */
export interface ViolationEventLike {
  readonly effectiveDirective?: unknown;
  readonly violatedDirective?: unknown;
  readonly disposition?: unknown;
}

/** Where violation events come from (the document). */
export interface ViolationTarget {
  addEventListener(
    type: 'securitypolicyviolation',
    listener: (event: Event) => void,
    options: AddEventListenerOptions,
  ): void;
  removeEventListener(
    type: 'securitypolicyviolation',
    listener: (event: Event) => void,
    options: EventListenerOptions,
  ): void;
}

/** Options of {@link startTrustedTypesCollector}. */
export interface TrustedTypesCollectorOptions {
  /**
   * Called the first time a directive is violated, with the directive and whether the policy only
   * reported it (`report`) or blocked it (`enforce`). The console by default.
   */
  readonly onNewDirective?: (directive: string, disposition: 'report' | 'enforce') => void;
}

/** A running collector. */
export interface TrustedTypesCollector {
  /** The counts so far. */
  report(): TrustedTypesReport;
  /** Stops listening; the counts stay readable. */
  stop(): void;
}

/** The directive an event was against, as a safe name. */
export function directiveOf(event: ViolationEventLike): string {
  const effective = event.effectiveDirective;
  const directive =
    typeof effective === 'string' && effective !== '' ? effective : event.violatedDirective;
  if (typeof directive !== 'string') {
    return OTHER_DIRECTIVE;
  }
  // `violatedDirective` may carry the directive's value too (`script-src 'self'`).
  const name = directive.trim().split(/\s/, 1)[0]?.toLowerCase() ?? '';
  return DIRECTIVE_NAME.test(name) ? name : OTHER_DIRECTIVE;
}

/** Whether the policy only reported the violation (`report`) or blocked it (`enforce`). */
function dispositionOf(event: ViolationEventLike): 'report' | 'enforce' {
  return event.disposition === 'report' ? 'report' : 'enforce';
}

function logNewDirective(directive: string, disposition: 'report' | 'enforce'): void {
  console.warn(
    `Content Security Policy violation (${disposition === 'report' ? 'reported only' : 'blocked'}): ${directive}`,
  );
}

/**
 * Starts counting the `securitypolicyviolation` events that reach `target` (listening in the
 * capture phase, so events fired at elements are seen as well as those fired at the document).
 */
export function startTrustedTypesCollector(
  target: ViolationTarget,
  options: TrustedTypesCollectorOptions = {},
): TrustedTypesCollector {
  const onNewDirective = options.onNewDirective ?? logNewDirective;
  const directives = new Set<string>();
  let count = 0;

  const listener = (event: Event): void => {
    count = Math.min(count + 1, Number.MAX_SAFE_INTEGER);
    const directive = directiveOf(event as ViolationEventLike);
    if (directives.has(directive) || directives.size >= MAX_DIRECTIVES) {
      return;
    }
    directives.add(directive);
    try {
      onNewDirective(directive, dispositionOf(event as ViolationEventLike));
    } catch {
      // Reporting must never break the page.
    }
  };
  target.addEventListener('securitypolicyviolation', listener, { capture: true, passive: true });

  let listening = true;
  return {
    report: () => ({ count, directives: [...directives].sort() }),
    stop: () => {
      if (listening) {
        listening = false;
        target.removeEventListener('securitypolicyviolation', listener, { capture: true });
      }
    },
  };
}
