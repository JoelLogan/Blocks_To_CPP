/**
 * The friendly g++ messages (docs/plans/m3-contracts/w4-editor-tools.md §2.6), against messages
 * recorded from GCC 13 (`LC_ALL=C`, straight quotes), with the curly quotes of a UTF-8 locale and
 * GCC 11's wording where they differ.
 */
import type { Diagnostic } from '@blocks2cpp/ipc-types';
import { describe, expect, it } from 'vitest';

import { diagnosticFixture } from '../../app/testing/fixtures';
import { GENERATOR_BUG_LABEL, labelGeneratorBugs } from './buildOutput';
import {
  compilerMessageText,
  friendlyCompilerMessage,
  quotedNames,
  warningOption,
} from './friendly';

/** A g++ diagnostic as the backend sends it: the code, and the original text. */
function gxx(code: string, raw: string, severity: Diagnostic['severity'] = 'warning'): Diagnostic {
  return diagnosticFixture({
    code,
    severity,
    source: code === 'C:link' ? 'linker' : 'compiler',
    message:
      severity === 'warning' ? `g++ warns: ${raw}` : `g++ reported a problem. It said: ${raw}`,
    raw,
  });
}

const RECORDED: readonly (readonly [Diagnostic, RegExp])[] = [
  [
    gxx(
      'C:-Wfloat-conversion',
      "main.cpp:8:11: conversion from 'double' to 'int' may change value [-Wfloat-conversion]",
    ),
    /^A double value is changed to int here, and may lose information/u,
  ],
  [
    gxx(
      'C:-Wsign-compare',
      "main.cpp:11:9: comparison of integer expressions of different signedness: 'int' and 'std::vector<int>::size_type' {aka 'long unsigned int'} [-Wsign-compare]",
    ),
    /may be negative is mixed here with a size/u,
  ],
  [
    gxx(
      'C:-Wsign-conversion',
      "main.cpp:12:16: conversion to 'unsigned int' from 'int' may change the sign of the result [-Wsign-conversion]",
    ),
    /may be negative/u,
  ],
  [
    gxx(
      'C:-Wshadow',
      "main.cpp:14:9: declaration of 'shadowed' shadows a previous local [-Wshadow]",
    ),
    /^The name “shadowed” here hides another one/u,
  ],
  [gxx('C:-Wdiv-by-zero', 'main.cpp:15:13: division by zero [-Wdiv-by-zero]'), /divides by zero/u],
  [
    gxx(
      'C:-Woverflow',
      "main.cpp:16:12: overflow in conversion from 'int' to 'char' changes value from '300' to '44' [-Woverflow]",
    ),
    /too large for its type/u,
  ],
  [
    gxx(
      'C:-Wdouble-promotion',
      "main.cpp:17:35: implicit conversion from 'float' to 'double' to match other operand of binary expression [-Wdouble-promotion]",
    ),
    /widened to a double/u,
  ],
  [
    gxx('C:-Wunused-variable', 'main.cpp:6:7: unused variable ‘unused’ [-Wunused-variable]'),
    /^“unused” is created but never used/u,
  ],
  [
    gxx(
      'C:-Wmaybe-uninitialized',
      "main.cpp:20:16: 'm' may be used uninitialized in this function [-Wmaybe-uninitialized]",
    ),
    /^“m” may be used before it has a value/u,
  ],
  [
    gxx(
      'C:-Werror=shadow',
      "main.cpp:3:5: declaration of 'x' shadows a parameter [-Werror=shadow]",
      'error',
    ),
    /^The name “x”/u,
  ],
];

describe('the rules', () => {
  it.each(RECORDED)('explain %#', (diagnostic, expected) => {
    expect(friendlyCompilerMessage(diagnostic)?.text).toMatch(expected);
  });

  it('find a header that is not installed, a program still running and a missing body', () => {
    const header = gxx(
      'C:error',
      'main.cpp:1:10: fatal error: nothere.h: No such file or directory',
      'error',
    );
    expect(friendlyCompilerMessage(header)).toEqual({
      text: 'The header “nothere.h” was not found. Check the name in the “use header” block, or install the library that has it.',
      notGeneratorBug: true,
    });
    const running = gxx(
      'C:link',
      '/usr/bin/ld: cannot open output file /home/a/b/game: Permission denied',
      'error',
    );
    expect(friendlyCompilerMessage(running)?.notGeneratorBug).toBe(true);
    const body = gxx('C:link', "main.cpp:(.text+0xe): undefined reference to `f(int)'", 'error');
    expect(friendlyCompilerMessage(body)).toEqual({
      text: '“f” is declared but has no body. Check its “define” block.',
      notGeneratorBug: false,
    });
    const print = gxx(
      'C:error',
      "main.cpp:21:22: no match for 'operator<<' (operand types are 'std::ostream' {aka 'std::basic_ostream<char>'} and 'Point')",
      'error',
    );
    expect(friendlyCompilerMessage(print)?.text).toMatch(/Tick “printable”/u);
  });

  it('leave other messages and other sources alone', () => {
    expect(
      friendlyCompilerMessage(
        gxx('C:-Wpedantic', 'main.cpp:1:1: ISO C++ forbids something [-Wpedantic]'),
      ),
    ).toBeNull();
    expect(friendlyCompilerMessage(diagnosticFixture())).toBeNull();
  });

  it('read the message, its quoted names and its option', () => {
    expect(
      compilerMessageText(
        "In file included from main.cpp:1:\nmain.cpp: In function 'int main()':\nmain.cpp:3:5: warning: x [-Wshadow]\nmain.cpp:2:5: note: y",
      ),
    ).toBe('x');
    expect(quotedNames("from ‘int’ to 'char' and `f(int)'")).toEqual(['int', 'char', 'f(int)']);
    expect(warningOption('C:-Werror=conversion')).toBe('-Wconversion');
    expect(warningOption('C:error')).toBeNull();
  });
});

describe('in the build output', () => {
  it('reads a warning as g++ warns and the friendly text, keeping the raw text', () => {
    const warning = RECORDED[3]?.[0];
    expect(warning).toBeDefined();
    if (warning === undefined) {
      return;
    }
    const [item] = labelGeneratorBugs([warning], null).items;
    expect(item?.message).toBe(
      'g++ warns: The name “shadowed” here hides another one with the same name. Give one of them another name.',
    );
    expect(item?.raw).toBe(warning.raw);
  });

  it('reads a missing header only from g++’s message at a place, as the backend sends it', () => {
    // The backend's raw has no severity word: `file:line:col: message`.
    const header = gxx('C:error', 'main.cpp:1:10: nothere.h: No such file or directory', 'error');
    expect(friendlyCompilerMessage(header)).toEqual({
      text: 'The header “nothere.h” was not found. Check the name in the “use header” block, or install the library that has it.',
      notGeneratorBug: true,
    });
    // The linker cannot find a library: not a header, and still a bug in Blocks2Cpp.
    const library = gxx('C:link', 'cannot find -lasan: No such file or directory', 'error');
    expect(friendlyCompilerMessage(library)).toBeNull();
    const labelled = labelGeneratorBugs([library], null);
    expect(labelled.generatorBug).toBe(true);
    expect(labelled.items[0]?.message).toContain(GENERATOR_BUG_LABEL);
    // The driver cannot find the source file: no place, so not a header either.
    const driver = gxx('C:error', 'main.cpp: No such file or directory', 'error');
    expect(friendlyCompilerMessage(driver)).toBeNull();
  });

  it('does not call a header that is not installed a bug, and keeps the label on a real one', () => {
    const header = gxx(
      'C:error',
      'main.cpp:1:10: fatal error: x.h: No such file or directory',
      'error',
    );
    const body = gxx('C:link', "main.cpp:(.text+0xe): undefined reference to `f(int)'", 'error');
    const result = labelGeneratorBugs([header], null);
    expect(result.generatorBug).toBe(false);
    expect(result.items[0]?.message).toMatch(/^The header “x.h” was not found/u);
    const bug = labelGeneratorBugs([body], null);
    expect(bug.generatorBug).toBe(true);
    expect(bug.items[0]?.message).toMatch(
      new RegExp(`^${GENERATOR_BUG_LABEL}\\. g\\+\\+ reported`, 'u'),
    );
    expect(bug.items[0]?.message).toMatch(/In plain words: “f” is declared but has no body/u);
  });
});
