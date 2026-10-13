/**
 * Opening a benchmark project (project.ts): a build that shows a block of the file as a placeholder
 * would measure another program, so the open fails (README.md, "A/B runs").
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { type Element as PageElement, Window } from 'happy-dom';
import { describe, expect, it } from 'vitest';

import { REPOSITORY_ROOT } from '../support/env';
import { runScript } from '../support/fake-page';
import {
  PLACEHOLDER_BLOCK_TYPE,
  PLACEHOLDERS_SCRIPT,
  requireEveryBlockRead,
  UnreadBlocksError,
} from './project';

/** An SVG group with class attribute `classes`, holding `children`. */
interface Group {
  readonly classes: string;
  readonly children?: readonly Group[];
}

/** Runs {@link PLACEHOLDERS_SCRIPT} on a page whose canvas holds the SVG groups `groups`. */
function placeholdersIn(groups: readonly Group[]): unknown {
  const window = new Window();
  try {
    const document = window.document;
    const svg = 'http://www.w3.org/2000/svg';
    const add = (parent: PageElement, group: Group): void => {
      const element = document.createElementNS(svg, 'g');
      element.setAttribute('class', group.classes);
      parent.append(element);
      for (const child of group.children ?? []) {
        add(element, child);
      }
    };
    const canvas = document.createElementNS(svg, 'svg');
    document.body.append(canvas);
    for (const group of groups) {
      add(canvas, group);
    }
    return runScript(PLACEHOLDERS_SCRIPT, { document }, []);
  } finally {
    void window.happyDOM.close();
  }
}

describe('PLACEHOLDERS_SCRIPT', () => {
  it('counts the placeholder blocks, by the type Blockly gives each block as a class', () => {
    expect(
      placeholdersIn([
        { classes: 'var.change blocklyDraggable' },
        {
          classes: `${PLACEHOLDER_BLOCK_TYPE} blocklyDraggable`,
          children: [{ classes: 'io.print' }],
        },
        { classes: `blocklyDraggable ${PLACEHOLDER_BLOCK_TYPE}` },
        { classes: 'b2c.placeholderish' },
      ]),
    ).toBe(2);
    expect(placeholdersIn([{ classes: 'var.change' }])).toBe(0);
  });

  it('looks for the placeholder type of the editor’s blocks', () => {
    const source = readFileSync(
      path.join(REPOSITORY_ROOT, 'packages/blockly-ext/src/blocks/placeholder.ts'),
      'utf8',
    );
    expect(source).toContain(`export const PLACEHOLDER_TYPE = '${PLACEHOLDER_BLOCK_TYPE}';`);
  });
});

describe('requireEveryBlockRead', () => {
  it('lets a project with no placeholders through', () => {
    expect(() => {
      requireEveryBlockRead(0);
    }).not.toThrow();
  });

  it('fails a project shown with placeholders, saying why', () => {
    expect(() => {
      requireEveryBlockRead(124);
    }).toThrow(UnreadBlocksError);
    expect(() => {
      requireEveryBlockRead(124);
    }).toThrow(/124 placeholder blocks.*block catalog is older/);
  });
});
