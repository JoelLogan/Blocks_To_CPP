/**
 * Vitest's global set-up of the E2E project: starts each run with an empty Trusted Types report and
 * writes its Markdown summary when the run ends (the CI job adds it to its summary).
 */
import { clearTrustedTypes, writeTrustedTypesSummary } from './artifacts';
import { harnessSettings } from './env';

export default function setup(): () => void {
  const { artifacts } = harnessSettings();
  clearTrustedTypes(artifacts);
  return () => {
    writeTrustedTypesSummary(artifacts);
  };
}
