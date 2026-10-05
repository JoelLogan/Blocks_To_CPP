/** The console header from the run slice, with the notices of 04 §4.5 and 08 §8.14. */
import { describe, expect, it } from 'vitest';

import { initialRun, type RunExitEvent, type RunState } from '../../app/store';
import { consoleHeaderFrom } from './consoleHeader';

const EXIT: RunExitEvent = {
  kind: 'exit',
  afterSeq: 4,
  elapsedMs: 61_000,
  status: { type: 'exited', code: 2 },
  crash: null,
  sanitizer: null,
  message: 'Finished with exit code 2',
};

function run(patch: Partial<RunState>): RunState {
  return { ...initialRun(), ...patch };
}

describe('consoleHeaderFrom', () => {
  it('is idle before any run', () => {
    expect(consoleHeaderFrom(initialRun(), 0)).toEqual({
      state: 'idle',
      exit: null,
      elapsedMs: 0,
      notices: [],
    });
  });

  it('shows the elapsed time and the notices while running', () => {
    expect(
      consoleHeaderFrom(
        run({
          status: 'running',
          startedAt: 1000,
          containment: 'processGroupOnly',
          ideHelpers: true,
        }),
        5200,
      ),
    ).toEqual({
      state: 'running',
      exit: null,
      elapsedMs: 4200,
      notices: ['ideHelpers', 'processGroupOnly'],
    });
    expect(consoleHeaderFrom(run({ status: 'running', startedAt: 9000 }), 5000).elapsedMs).toBe(0);
    expect(consoleHeaderFrom(run({ status: 'running' }), 5000).elapsedMs).toBe(0);
  });

  it('shows no containment notice for a cgroup or a Job Object', () => {
    for (const containment of ['cgroup', 'jobObject'] as const) {
      expect(
        consoleHeaderFrom(run({ status: 'running', startedAt: 0, containment }), 0).notices,
      ).toEqual([]);
    }
  });

  it('shows the exit with the backend elapsed time, also while the next run starts', () => {
    const exited = consoleHeaderFrom(
      run({ status: 'exited', exit: EXIT, containment: 'processGroupOnly' }),
      0,
    );
    expect(exited).toEqual({
      state: 'exited',
      exit: EXIT,
      elapsedMs: 61_000,
      notices: ['processGroupOnly'],
    });
    expect(consoleHeaderFrom(run({ status: 'starting', exit: EXIT }), 0).state).toBe('exited');
    expect(consoleHeaderFrom(run({ status: 'starting' }), 0).state).toBe('idle');
    expect(consoleHeaderFrom(run({ status: 'exited' }), 0).state).toBe('idle');
  });
});
