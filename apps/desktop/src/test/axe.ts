/**
 * Automated accessibility checks for component tests (docs/spec/09-quality-and-delivery.md §9.2,
 * docs/spec/04-user-interface.md §4.8): every panel and dialog test calls
 * {@link expectNoAxeViolations} on what it rendered.
 */
import axe from 'axe-core';
import { assert } from 'vitest';

/** The standard the app meets: WCAG 2.2 level AA (and everything below it), plus axe's best practices. */
const TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'];

/**
 * Rules that cannot give a reliable answer in happy-dom, which computes no layout. They are covered
 * by the end-to-end tests in the real webviews and the manual accessibility pass instead.
 */
const NEEDS_LAYOUT = ['color-contrast', 'color-contrast-enhanced', 'target-size'];

/** Options for {@link expectNoAxeViolations}. */
export interface AxeCheckOptions {
  /** Rules to skip as well. Say why at the call site; prefer fixing the markup. */
  readonly skipRules?: readonly string[];
}

/**
 * Runs axe-core on `container` (which must be attached to the document, as Testing Library's
 * `render` does) and fails the test with a readable list if it finds any violation of WCAG 2.2 AA
 * or of axe's best practices. Rules about the whole page (a `lang` attribute, the title, landmarks
 * around all content) do not apply to a container; the app shell's test checks its landmarks.
 */
export async function expectNoAxeViolations(
  container: Element,
  options: AxeCheckOptions = {},
): Promise<void> {
  const skipped = [...NEEDS_LAYOUT, ...(options.skipRules ?? [])];
  const results = await axe.run(container, {
    runOnly: { type: 'tag', values: TAGS },
    rules: Object.fromEntries(skipped.map((id) => [id, { enabled: false }])),
    resultTypes: ['violations'],
  });

  if (results.violations.length > 0) {
    const problems = results.violations.map(describeViolation).join('\n');
    assert.fail(
      `axe-core found ${String(results.violations.length)} accessibility problem(s):\n${problems}`,
    );
  }
}

/** One line per problem, then where it is: enough to find and fix it without rerunning axe. */
function describeViolation(violation: axe.Result): string {
  const where = violation.nodes.map((node) => `    at ${node.target.join(' ')}`);
  return [
    `  ${violation.id} (${violation.impact ?? 'unknown impact'}): ${violation.help}`,
    `    see ${violation.helpUrl}`,
    ...where,
  ].join('\n');
}
