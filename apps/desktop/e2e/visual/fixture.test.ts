/** The visual diff's project (fixture.ts). */
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  FIXTURE_VIEWPORT,
  fixtureText,
  GUESSING_GAME,
  GUESSING_GAME_MAIN,
  writeFixture,
} from './fixture';

const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

interface Doc {
  modules: { workspace: { blocks: { id: string; type: string }[]; viewport?: unknown } }[];
}

describe('fixtureText', () => {
  it('is the guessing game with a fixed view at zoom 1.0', () => {
    const example = JSON.parse(readFileSync(GUESSING_GAME, 'utf8')) as Doc;
    const fixture = JSON.parse(fixtureText()) as Doc;
    expect(fixture.modules[0]?.workspace.viewport).toEqual(FIXTURE_VIEWPORT);
    expect(FIXTURE_VIEWPORT.scale).toBe(1);
    // Nothing else changes.
    const without = structuredClone(fixture);
    delete without.modules[0]?.workspace.viewport;
    expect(without).toEqual(example);
  });

  it('names the program block of the example', () => {
    const example = JSON.parse(readFileSync(GUESSING_GAME, 'utf8')) as Doc;
    const top = example.modules[0]?.workspace.blocks ?? [];
    expect(
      top.filter((block) => block.id === GUESSING_GAME_MAIN).map((block) => block.type),
    ).toEqual(['program.main']);
  });

  it('refuses a document that is not a one-module project', () => {
    expect(() => fixtureText('null')).toThrow(/not a project/);
    expect(() => fixtureText('{"modules": []}')).toThrow(/exactly one module/);
    expect(() => fixtureText('{"modules": [{}]}')).toThrow(/no workspace/);
  });
});

describe('writeFixture', () => {
  it('writes the project file', () => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-visual-fixture-'));
    folders.push(folder);
    const file = writeFixture(folder);
    expect(readFileSync(file, 'utf8')).toBe(fixtureText());
  });
});
