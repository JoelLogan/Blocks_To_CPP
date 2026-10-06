/**
 * Vitest's global set-up of the visual diff: starts each run with an empty visual summary and, when
 * the run ends, writes the Trusted Types summary again from the report. The report itself is not
 * cleared (unlike support/global.ts): the visual diff runs after the other end-to-end tests in the
 * same CI job, and the job's summary shows the Trusted Types counts of all of them.
 */
import { rmSync } from 'node:fs';
import path from 'node:path';

import { writeTrustedTypesSummary } from '../support/artifacts';
import { harnessSettings } from '../support/env';
import { VISUAL_SUMMARY, visualSettings } from './baselines';

export default function setup(): () => void {
  rmSync(path.join(visualSettings().out, VISUAL_SUMMARY), { force: true });
  return () => {
    writeTrustedTypesSummary(harnessSettings().artifacts);
  };
}
