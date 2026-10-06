/**
 * Webview benchmark: cold start (docs/spec/01-overview.md §1.4 N2, 09 §9.2). The app is started
 * {@link LAUNCHES} times, each with a fresh profile, and each start is timed from the WebDriver
 * session request (which starts the app) to the readiness marker: the start page shown, Blockly's
 * workspace injected and the toolbox rendered (bench/page.ts `READY_SCRIPT`). A first start warms
 * the system's file caches and is not counted. The samples go to `webview.cold-start.json`
 * (bench/results.ts) for tools/bench-compare.py.
 */
import { existsSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { harnessSettings } from '../support/env';
import { measureColdStart } from './launch';
import { writeMetric } from './results';
import { median, rounded } from './stats';

/** How many timed starts make the metric (the comparison needs at least 10 samples). */
const LAUNCHES = 10;

describe('webview benchmark: cold start', () => {
  it(`times ${String(LAUNCHES)} starts of the app to the editor's readiness marker`, async () => {
    const settings = harnessSettings();
    if (!existsSync(settings.app)) {
      throw new Error(`The app under test is missing: ${settings.app} (set B2C_E2E_APP)`);
    }
    await measureColdStart(settings, 'cold-start-warm-up');
    const samples: number[] = [];
    let upperBounds = 0;
    let restarted = 0;
    for (let launch = 1; launch <= LAUNCHES; launch += 1) {
      const start = await measureColdStart(settings, `cold-start-${String(launch)}`);
      samples.push(start.ms);
      if (start.already) {
        upperBounds += 1;
      }
      if (start.restarts > 0) {
        restarted += 1;
      }
    }
    const file = writeMetric({
      name: 'webview.cold-start',
      unit: 'ms',
      better: 'lower',
      gate: true,
      description: `Session request to start page, injected workspace and toolbox; ${String(LAUNCHES)} fresh starts`,
      samples,
    });
    process.stdout.write(
      `Cold start: median ${String(rounded(median(samples), 1))} ms over ${String(LAUNCHES)} starts ` +
        `(${String(upperBounds)} already ready when first asked, ${String(restarted)} waited again ` +
        `after the window replaced its page); written to ${file}\n`,
    );
    expect(samples).toHaveLength(LAUNCHES);
  });
});
