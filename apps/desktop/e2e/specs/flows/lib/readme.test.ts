/**
 * The flows README's table lists exactly the flow specs of `specs/flows/`, one row each, so a new
 * flow cannot be left out of it.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

/** `specs/flows/`. */
const FLOWS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** The suffix of a flow spec. */
const SPEC = '.e2e.ts';

/** The flow names of the README's table rows (`` | [`name`](name.e2e.ts) | … ``), in order. */
function readmeRows(): string[] {
  const readme = readFileSync(path.join(FLOWS, 'README.md'), 'utf8');
  const rows: string[] = [];
  for (const line of readme.split('\n')) {
    const match = /^\| \[`([^`]+)`\]\(([^)]+)\)/.exec(line);
    const [, name = '', link = ''] = match ?? [];
    if (match) {
      expect(link, `the row of ${name} links its own spec`).toBe(`${name}${SPEC}`);
      rows.push(name);
    }
  }
  return rows;
}

describe('the flows README', () => {
  it('has one row for each flow spec', () => {
    const specs = readdirSync(FLOWS)
      .filter((name) => name.endsWith(SPEC))
      .map((name) => name.slice(0, -SPEC.length))
      .sort();
    const rows = readmeRows();
    expect(new Set(rows).size, 'a flow has two rows').toBe(rows.length);
    expect([...rows].sort()).toEqual(specs);
  });
});
