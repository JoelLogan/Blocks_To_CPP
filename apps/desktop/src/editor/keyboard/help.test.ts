import checklist from '../../../../../docs/manual-tests/m2-accessibility.md?raw';
import { describe, expect, it } from 'vitest';

import { KEY_MAP, type KeyScope } from './help';

/** The checklist's heading over each scope's table. */
const SECTIONS: Readonly<Record<KeyScope, string>> = {
  canvas: '### On the canvas',
  move: '### While moving a block',
  toolbox: '### In the toolbox',
  flyout: "### In the toolbox's blocks",
};

/** The rows of the table under `heading`, as `[keys, action]`. */
function tableRows(heading: string): string[][] {
  const start = checklist.indexOf(`${heading}\n`);
  expect(start, `the checklist has no "${heading}"`).toBeGreaterThanOrEqual(0);
  const rest = checklist.slice(start + heading.length + 1);
  const end = rest.search(/\n#{2,3} /);
  return (end < 0 ? rest : rest.slice(0, end))
    .split('\n')
    .filter(
      (line) => line.startsWith('| ') && !line.startsWith('| ---') && line !== '| Keys | Action |',
    )
    .map((line) =>
      line
        .slice(1, -1)
        .split(' | ')
        .map((cell) => cell.trim()),
    );
}

describe('the manual accessibility checklist', () => {
  it('lists exactly the key map, scope by scope (docs/manual-tests/m2-accessibility.md)', () => {
    for (const [scope, heading] of Object.entries(SECTIONS) as [KeyScope, string][]) {
      const expected = KEY_MAP.filter((row) => row.scope === scope).map((row) => [
        row.keys,
        row.action,
      ]);
      expect(tableRows(heading)).toEqual(expected);
    }
  });
});
