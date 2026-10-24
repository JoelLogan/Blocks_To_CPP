/**
 * The visual diff's project: the guessing game (examples/guessing_game.b2c, 03 §3.13.1) with a fixed
 * view of its module, so the canvas shows the same part of it at zoom 1.0 every time.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { REPOSITORY_ROOT } from '../support/env';

/** The example the visual diff shows. */
export const GUESSING_GAME = path.join(REPOSITORY_ROOT, 'examples', 'guessing_game.b2c');

/** The guessing game's `program.main` block. */
export const GUESSING_GAME_MAIN = 'b011';

/**
 * The view of the module (05 §5.3 `viewport`): workspace coordinates of the visible canvas's
 * top-left corner, and the zoom. The toolbox's flyout stays open over the canvas's left part, so
 * the view starts left of the program to show it beside the flyout.
 */
export const FIXTURE_VIEWPORT = { x: 0, y: 0, scale: 1 } as const;

/** The project text: the example with {@link FIXTURE_VIEWPORT} set on its one module. */
export function fixtureText(example: string = readFileSync(GUESSING_GAME, 'utf8')): string {
  const document: unknown = JSON.parse(example);
  if (typeof document !== 'object' || document === null) {
    throw new Error('The guessing game example is not a project');
  }
  const modules = (document as { modules?: unknown }).modules;
  if (!Array.isArray(modules) || modules.length !== 1) {
    throw new Error('The guessing game example must have exactly one module');
  }
  const workspace = (modules[0] as { workspace?: unknown }).workspace;
  if (typeof workspace !== 'object' || workspace === null) {
    throw new Error('The guessing game example has no workspace');
  }
  (workspace as Record<string, unknown>)['viewport'] = { ...FIXTURE_VIEWPORT };
  return `${JSON.stringify(document, null, 2)}\n`;
}

/** Writes the fixture project into `folder` and returns its path. */
export function writeFixture(folder: string): string {
  const file = path.join(folder, 'guessing_game.b2c');
  writeFileSync(file, fixtureText());
  return file;
}
