/**
 * The preview worker's probe (ADR-0016 §6), in end-to-end builds only: which policy the worker runs
 * under. The page's policy and the worker's own (`WORKER_CSP`) both refuse `eval`, so `eval` alone
 * only shows that some policy applies; a request to the IPC endpoint tells them apart, since the
 * page's `connect-src` allows it and the worker's policy allows no request at all. The test
 * hook makes the same request from the page as the control (src/e2e/workerCheck.ts): an endpoint
 * that answers the page but not the worker was refused by the worker's own policy.
 */
import type { WorkerProbe } from '@blocks2cpp/b2c-core-wasm/worker';

/**
 * The IPC endpoints the page's `connect-src` allows (`ipc: http://ipc.localhost`): WebKitGTK's
 * custom scheme and WebView2's host. A plain GET, which the app's IPC refuses without running
 * anything.
 */
export const IPC_PROBE_URLS = ['ipc://localhost/b2c-probe', 'http://ipc.localhost/b2c-probe'];

/** How long to wait for violation events after the requests have settled. */
const SETTLE_MS = 100;

/** A directive's name, as the page's collector accepts it (src/lib/trustedTypes.ts). */
const DIRECTIVE_NAME = /^[a-z][a-z-]{0,63}$/;

/** The directives that would block a request. */
const REQUEST_DIRECTIVES = new Set(['connect-src', 'default-src']);

/** The members of the worker's global scope the probe uses. */
export interface ProbeScope {
  addEventListener(type: 'securitypolicyviolation', listener: (event: Event) => void): void;
}

/**
 * Requests each of {@link IPC_PROBE_URLS}: `allowed` when one of them answered (whatever its
 * status), else `blocked` when a request directive was reported violated, else `failed`.
 */
export async function probeRequest(
  violated: () => readonly string[],
  request: (url: string) => Promise<unknown> = (url) => fetch(url),
): Promise<WorkerProbe['request']> {
  let answered = false;
  for (const url of IPC_PROBE_URLS) {
    try {
      await request(url);
      answered = true;
    } catch {
      // Refused by a policy, or nothing answers there on this system.
    }
  }
  await new Promise((resolve) => setTimeout(resolve, SETTLE_MS));
  if (answered) {
    return 'allowed';
  }
  return violated().some((directive) => REQUEST_DIRECTIVES.has(directive)) ? 'blocked' : 'failed';
}

/** Whether `eval` runs here. */
export function probeEval(): WorkerProbe['eval'] {
  try {
    // Indirect, as a bundler wants it; a policy without 'unsafe-eval' refuses it all the same.
    // eslint-disable-next-line no-eval -- the probe checks that the worker's policy blocks eval
    (0, eval)('1');
    return 'allowed';
  } catch {
    return 'blocked';
  }
}

/**
 * Starts listening for violations now (the worker's first module) and returns the probe. Nothing
 * runs until the probe is called; then it records only directive names, never a sample or a URL.
 */
export function workerProbe(scope: ProbeScope): () => Promise<WorkerProbe> {
  const directives = new Set<string>();
  scope.addEventListener('securitypolicyviolation', (event) => {
    const { effectiveDirective } = event as Event & { effectiveDirective?: unknown };
    if (typeof effectiveDirective === 'string' && DIRECTIVE_NAME.test(effectiveDirective)) {
      directives.add(effectiveDirective);
    }
  });

  return async () => {
    const evaluated = probeEval();
    const request = await probeRequest(() => [...directives]);
    return { eval: evaluated, request, directives: [...directives].sort() };
  };
}
