import { describe, expect, it } from 'vitest';

import textRs from '../../../../../crates/b2c-ir/src/text.rs?raw';
import { CPP_KEYWORDS, variableNameProblem } from './identifiers';

/** The string entries of a Rust `const NAME: &[&str] = &[...]` in crates/b2c-ir/src/text.rs. */
function rustStringList(source: string, name: string): string[] {
  const start = source.indexOf(`const ${name}: &[&str] = &[`);
  expect(start).toBeGreaterThanOrEqual(0);
  const end = source.indexOf('];', start);
  return [...source.slice(start, end).matchAll(/"([^"]*)"/g)].map((match) => match[1] ?? '');
}

const NONE = new Set<string>();

describe('Make a variable name check', () => {
  it('uses the same keyword list as the analyser (b2c-ir KEYWORDS)', () => {
    expect([...CPP_KEYWORDS].sort()).toEqual(rustStringList(textRs, 'KEYWORDS').sort());
  });

  it('reserves the same names as the analyser (b2c-ir GENERATOR_RESERVED and b2c prefix)', () => {
    for (const name of rustStringList(textRs, 'GENERATOR_RESERVED')) {
      expect(variableNameProblem(name, NONE)).toMatch(/reserved by Blocks2Cpp/);
    }
    expect(textRs).toContain('const GENERATED_PREFIX: &str = "b2c";');
    for (const name of ['b2c', 'b2cTemp', 'B2C_value', 'B2c']) {
      expect(variableNameProblem(name, NONE)).toMatch(/reserved by Blocks2Cpp/);
    }
  });

  it('accepts ordinary names', () => {
    for (const name of ['score', 'x', 'value2', 'my_score', 'A', 'a'.repeat(64), 'iNT']) {
      expect(variableNameProblem(name, NONE)).toBeNull();
    }
  });

  it('explains each rule a name breaks', () => {
    expect(variableNameProblem('', NONE)).toMatch(/Type a name/);
    expect(variableNameProblem('a'.repeat(65), NONE)).toMatch(/at most 64 characters/);
    expect(variableNameProblem('my score', NONE)).toMatch(/letters .*digits and underscores/);
    expect(variableNameProblem('größe', NONE)).toMatch(/letters .*digits and underscores/);
    expect(variableNameProblem('x‮', NONE)).toMatch(/letters .*digits and underscores/);
    expect(variableNameProblem('2fast', NONE)).toMatch(/start with a letter/);
    expect(variableNameProblem('_hidden', NONE)).toMatch(/start with a letter/);
    expect(variableNameProblem('a__b', NONE)).toMatch(/two underscores/);
    expect(variableNameProblem('int', NONE)).toMatch(/“int” is a C\+\+ keyword/);
    expect(variableNameProblem('and', NONE)).toMatch(/keyword/);
    expect(variableNameProblem('main', NONE)).toMatch(/reserved/);
  });

  it('refuses a name that is already in use where the variable goes', () => {
    expect(variableNameProblem('guess', new Set(['guess', 'secret']))).toMatch(
      /already something called “guess”/,
    );
    expect(variableNameProblem('Guess', new Set(['guess']))).toBeNull();
  });
});
