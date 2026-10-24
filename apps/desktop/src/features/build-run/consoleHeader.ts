/**
 * The console header for the run slice of the app's state (docs/spec/04-user-interface.md §4.5):
 * the state (`Running`, then the exit text), the elapsed time and the notices *Running with IDE
 * helpers* and *Process group only* (08 §8.14: on Linux without cgroup v2, a program that
 * daemonises can outlive Stop).
 */
import type { RunState } from '../../app/store';
import type { ConsoleHeader, ConsoleNotice } from '../../panels';

const NO_NOTICES: readonly ConsoleNotice[] = Object.freeze([]);

/** The notices of the run the header describes. */
function noticesOf(run: RunState): readonly ConsoleNotice[] {
  const notices: ConsoleNotice[] = [];
  if (run.ideHelpers) {
    notices.push('ideHelpers');
  }
  if (run.containment === 'processGroupOnly') {
    notices.push('processGroupOnly');
  }
  return notices.length === 0 ? NO_NOTICES : notices;
}

/**
 * The header for `run` at time `now` (`Date.now()`). While a new run is starting (building, or
 * waiting for the program), the header keeps showing how the last run ended, such as *Stopped*
 * when Run stopped a running program; with no earlier run it shows *Not running*.
 */
export function consoleHeaderFrom(run: RunState, now: number): ConsoleHeader {
  switch (run.status) {
    case 'running':
      return {
        state: 'running',
        exit: null,
        elapsedMs: run.startedAt === null ? 0 : Math.max(0, now - run.startedAt),
        notices: noticesOf(run),
      };
    case 'exited':
    case 'starting':
      if (run.exit !== null) {
        return {
          state: 'exited',
          exit: run.exit,
          elapsedMs: run.exit.elapsedMs,
          notices: noticesOf(run),
        };
      }
      return { state: 'idle', exit: null, elapsedMs: 0, notices: NO_NOTICES };
    case 'idle':
      return { state: 'idle', exit: null, elapsedMs: 0, notices: NO_NOTICES };
  }
}
