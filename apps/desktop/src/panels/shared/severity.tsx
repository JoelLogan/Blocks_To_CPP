/**
 * How a severity looks everywhere in the panels: an icon and a word, never colour alone
 * (docs/spec/04-user-interface.md §4.4, §4.8).
 */
import type { Severity } from '../types';

/** The icon of each severity (04 §4.4: ✖ error, ⚠ warning, ℹ info). */
export const SEVERITY_ICON: Readonly<Record<Severity, string>> = {
  error: '✖',
  warning: '⚠',
  info: 'ℹ',
};

/** The word for each severity. */
export const SEVERITY_WORD: Readonly<Record<Severity, string>> = {
  error: 'Error',
  warning: 'Warning',
  info: 'Info',
};

/** Higher is more serious: for sorting and for picking the strongest of several. */
export const SEVERITY_RANK: Readonly<Record<Severity, number>> = {
  info: 1,
  warning: 2,
  error: 3,
};

/** The more serious of two severities. */
export function strongerSeverity(a: Severity, b: Severity): Severity {
  return SEVERITY_RANK[b] > SEVERITY_RANK[a] ? b : a;
}

/** A severity's icon (hidden from screen readers, which read the word) and its word. */
export function SeverityLabel({ severity }: { severity: Severity }) {
  return (
    <span className={`b2c-severity b2c-severity-${severity}`}>
      <span className="b2c-severity-icon" aria-hidden="true">
        {SEVERITY_ICON[severity]}
      </span>{' '}
      {SEVERITY_WORD[severity]}
    </span>
  );
}
