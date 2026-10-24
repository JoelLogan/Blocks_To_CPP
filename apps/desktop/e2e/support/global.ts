/**
 * Vitest's global set-up of the E2E project: starts each run with an empty Trusted Types report and
 * an empty long-task report, and writes their Markdown summaries when the run ends (the CI jobs add
 * them to their summaries).
 */
import { clearTrustedTypes, writeTrustedTypesSummary } from './artifacts';
import { harnessSettings } from './env';
import { clearLongTasks, writeLongTaskSummary } from './longtasks';

export default function setup(): () => void {
  const { artifacts } = harnessSettings();
  clearTrustedTypes(artifacts);
  clearLongTasks(artifacts);
  return () => {
    writeTrustedTypesSummary(artifacts);
    writeLongTaskSummary(artifacts);
  };
}
