/** The Build output lines and the generator-bug label. */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import { describe, expect, it } from 'vitest';

import { diagnosticFixture, documentFixture } from '../../app/testing/fixtures';
import {
  buildStartLine,
  diagnosticLines,
  finishedLine,
  formatDuration,
  GENERATOR_BUG_LABEL,
  labelGeneratorBugs,
  MAX_NAME_CHARS,
  MAX_RAW_LINES_PER_DIAGNOSTIC,
  progressLine,
  rawBlockIds,
} from './buildOutput';

function documentWith(blocks: BdmBlock[]) {
  const document = documentFixture();
  document.modules = [{ id: 'mod_main', name: 'main', workspace: { blocks } }];
  return document;
}

describe('the build output lines', () => {
  it('formats durations', () => {
    expect(formatDuration(0)).toBe('0.0 s');
    expect(formatDuration(440)).toBe('0.4 s');
    expect(formatDuration(12_345)).toBe('12.3 s');
    expect(formatDuration(125_000)).toBe('2 min 5 s');
    expect(formatDuration(Number.NaN)).toBe('0.0 s');
    expect(formatDuration(-5)).toBe('0.0 s');
  });

  it('starts with the project and the configuration, shortening a long name', () => {
    expect(buildStartLine('Guessing Game', 'debug')).toEqual({
      kind: 'progress',
      text: 'Building Guessing Game (Debug)…',
    });
    const long = 'n'.repeat(MAX_NAME_CHARS + 10);
    expect(buildStartLine(long, 'release').text).toBe(
      `Building ${'n'.repeat(MAX_NAME_CHARS)}… (Release)…`,
    );
  });

  it('names each stage', () => {
    expect(progressLine('generate', 1, 1).text).toBe('Generating C++ (1/1)');
    expect(progressLine('compile', 2, 3).text).toBe('Compiling (2/3)');
    expect(progressLine('link', 1, 1).text).toBe('Linking (1/1)');
  });

  it('says how every outcome ended', () => {
    expect(finishedLine('built', 1200, 0).text).toBe('Built in 1.2 s.');
    expect(finishedLine('upToDate', 5, 0).text).toBe(
      'Up to date: nothing changed since the last build.',
    );
    expect(finishedLine('projectErrors', 5, 2).text).toBe(
      'Not built: the blocks have errors (see Problems).',
    );
    expect(finishedLine('toolchainProblem', 5, 1).text).toBe(
      'Not built: there is a problem with the C++ compiler (see Problems).',
    );
    expect(finishedLine('cancelled', 5, 0).text).toBe('Build stopped.');
    expect(finishedLine('failed', 2000, 2).text).toBe(
      'Build failed after 2.0 s: 2 errors (see Problems).',
    );
    expect(finishedLine('failed', 2000, 0).text).toBe('Build failed after 2.0 s.');
  });

  it('shows the compiler text line by line and the other diagnostics as notes', () => {
    const lines = diagnosticLines([
      diagnosticFixture({ code: 'B2C-T1013', source: 'toolchain', message: 'Linked dynamically.' }),
      diagnosticFixture({
        code: 'C:error',
        source: 'compiler',
        raw: 'main.cpp:1:1: error: a\r\n  note: b\n\n',
      }),
      diagnosticFixture({ code: 'C:link', source: 'linker', message: 'undefined reference' }),
      diagnosticFixture({ code: 'B2C-E0201', source: 'analyser', message: 'No such variable.' }),
    ]);
    expect(lines).toEqual([
      { kind: 'note', text: 'B2C-T1013: Linked dynamically.' },
      { kind: 'raw', text: 'main.cpp:1:1: error: a' },
      { kind: 'raw', text: '  note: b' },
      { kind: 'raw', text: 'C:link: undefined reference' },
      { kind: 'note', text: 'B2C-E0201: No such variable.' },
    ]);
  });

  it('cuts a very long compiler message', () => {
    const raw = Array.from({ length: MAX_RAW_LINES_PER_DIAGNOSTIC + 2 }, (_, i) => `l${String(i)}`);
    const lines = diagnosticLines([
      diagnosticFixture({ code: 'C:error', source: 'compiler', raw: raw.join('\n') }),
    ]);
    expect(lines).toHaveLength(MAX_RAW_LINES_PER_DIAGNOSTIC + 1);
    expect(lines.at(-1)?.text).toBe('… 2 more lines');
  });
});

describe('labelGeneratorBugs', () => {
  const blocks: BdmBlock[] = [
    {
      id: 'b001',
      type: 'program.main',
      v: 1,
      statements: {
        BODY: [
          {
            id: 'b002',
            type: 'io.print',
            v: 1,
            inputs: { VALUE: { block: { id: 'b003', type: 'raw.expr', v: 1 } } },
          },
        ],
      },
    },
    {
      id: 'b004',
      type: 'io.print',
      v: 1,
      x: 0,
      y: 0,
      stack: [{ id: 'b005', type: 'raw.statement', v: 1 }],
    },
  ];

  it('finds raw blocks anywhere: inputs, statement lists and loose stacks', () => {
    expect([...rawBlockIds(documentWith(blocks))].sort()).toEqual(['b003', 'b005']);
  });

  it('labels C: errors of blocks that are not raw, once', () => {
    const compilerError = (block: string | undefined) =>
      diagnosticFixture({
        code: 'C:error',
        source: 'compiler',
        message: 'g++ said no',
        primary:
          block === undefined ? { part: { kind: 'whole' } } : { block, part: { kind: 'whole' } },
      });
    const warning = diagnosticFixture({ code: 'C:-Wall', severity: 'warning', source: 'compiler' });
    const analyser = diagnosticFixture();
    const result = labelGeneratorBugs(
      [compilerError('b002'), compilerError('b003'), compilerError(undefined), warning, analyser],
      documentWith(blocks),
    );
    expect(result.generatorBug).toBe(true);
    expect(result.items.map((item) => item.message)).toEqual([
      `${GENERATOR_BUG_LABEL}. g++ said no`,
      'g++ said no',
      `${GENERATOR_BUG_LABEL}. g++ said no`,
      warning.message,
      analyser.message,
    ]);

    const already = diagnosticFixture({
      code: 'C:link',
      source: 'linker',
      message: `The linker reported a problem. ${GENERATOR_BUG_LABEL}; please report it.`,
    });
    const again = labelGeneratorBugs([already], null);
    expect(again.generatorBug).toBe(true);
    expect(again.items[0]).toBe(already);
  });

  it('leaves diagnostics alone when there is no compiler error', () => {
    const items = [diagnosticFixture()];
    expect(labelGeneratorBugs(items, documentWith(blocks))).toEqual({ items, generatorBug: false });
  });
});
