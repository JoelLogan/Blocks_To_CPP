/**
 * Test support (not exported by the package): catalog definitions shaped like the blocks of
 * milestone M3's wave W2 (docs/plans/m3-contracts/w2.md §1), which the catalog does not have yet.
 * The mutator tests add them to the generated catalog with `vi.mock`, so the foundations of
 * w2.md §2.1 (repeated fields, joining words, flag-only branches, call arguments named by a
 * struct, row extras) are tested before the packages add the real blocks. The definitions follow
 * w2.md's tables; the packages' catalog files are the normative ones.
 */
import type {
  BlockDefJson,
  ExtraDefJson,
  FieldDefJson,
  InputDefJson,
  LabelPart,
  StatementDefJson,
} from '../generated/catalog';

/** Parses a friendly label as catalog-gen does (`%NAME`, a bare `…`, words between). */
export function labelParts(label: string): LabelPart[] {
  const parts: LabelPart[] = [];
  let words: string[] = [];
  const flush = (): void => {
    if (words.length > 0) {
      parts.push({ text: words.join(' ') });
      words = [];
    }
  };
  for (const word of label.split(' ')) {
    if (word.startsWith('%')) {
      flush();
      parts.push({ arg: word.slice(1) });
    } else if (word === '…') {
      flush();
      parts.push({ repeat: true });
    } else if (word !== '') {
      words.push(word);
    }
  }
  flush();
  return parts;
}

function field(name: string, kind: FieldDefJson['kind'], more: Partial<FieldDefJson> = {}) {
  return {
    name,
    kind,
    options: [],
    types: [],
    default: null,
    type: null,
    repeat: null,
    ...more,
  } satisfies FieldDefJson;
}

function input(name: string, more: Partial<InputDefJson> = {}): InputDefJson {
  return {
    name,
    check: 'any',
    optional: false,
    repeat: null,
    default: [],
    type: null,
    assign: false,
    ...more,
  };
}

function statement(name: string, more: Partial<StatementDefJson> = {}): StatementDefJson {
  return { name, repeat: null, when: null, ...more };
}

function count(name: string, min: number, max: number, value: number): ExtraDefJson {
  return { name, kind: 'count', min, max, default: value, types: [], typeClass: null };
}

function flag(name: string, value: boolean): ExtraDefJson {
  return { name, kind: 'flag', min: null, max: null, default: value, types: [], typeClass: null };
}

function block(
  id: string,
  category: BlockDefJson['category'],
  shape: BlockDefJson['shape'],
  friendly: string,
  more: Partial<BlockDefJson>,
): BlockDefJson {
  const parts = labelParts(friendly);
  const args = new Set(parts.flatMap((part) => ('arg' in part ? [part.arg] : [])));
  const statements = more.statements ?? [];
  return {
    id,
    version: 1,
    category,
    shape,
    label: { friendly, cpp: id },
    labelParts: parts,
    statementsNotInLabel: statements.map((s) => s.name).filter((name) => !args.has(name)),
    help: `Help for ${id}.`,
    output: null,
    fields: [],
    inputs: [],
    statements,
    extra: [],
    typeVars: {},
    order: [],
    placeVar: null,
    members: {},
    headers: [],
    optionHeaders: {},
    ...more,
  };
}

const ERRORS = ['std::exception', 'std::runtime_error', 'std::invalid_argument'];
const M2_TYPES = ['int', 'double', 'bool', 'char', 'std::string'];
const CATCH_REPEAT = { count: 'catchCount', plus: 0 };
const CASE_REPEAT = { count: 'caseCount', plus: 0 };

/** W2-shaped definitions, by ID (w2.md §1.5 to §1.8). */
export const W2_DEFS: readonly BlockDefJson[] = [
  block('error.try', 'errors', 'statement', 'try %TRY catch %ERROR as %NAME %CATCH catch …', {
    fields: [
      field('ERROR', 'type', { types: ERRORS, default: 'std::exception', repeat: CATCH_REPEAT }),
      field('NAME', 'symbol_decl', {
        type: { kind: 'field', field: 'ERROR' },
        repeat: CATCH_REPEAT,
      }),
    ],
    statements: [statement('TRY'), statement('CATCH', { repeat: CATCH_REPEAT })],
    extra: [count('catchCount', 1, 8, 1)],
    order: [{ field: 'ERROR' }, { field: 'NAME' }],
  }),
  block(
    'control.switch',
    'control',
    'statement',
    'switch on %VALUE case %CASE %DO case … default %DEFAULT',
    {
      inputs: [input('VALUE'), input('CASE', { repeat: CASE_REPEAT })],
      statements: [
        statement('DO', { repeat: CASE_REPEAT }),
        statement('DEFAULT', { when: 'hasDefault' }),
      ],
      extra: [count('caseCount', 1, 64, 2), flag('hasDefault', true)],
    },
  ),
  block(
    'file.for_each_line',
    'files',
    'statement',
    "for each line %LINE in file %PATH %BODY if it can't be opened %FAILED",
    {
      fields: [field('LINE', 'symbol_decl')],
      inputs: [input('PATH', { check: 'text', default: [{ str: 'data.txt' }] })],
      statements: [statement('BODY'), statement('FAILED', { when: 'hasFailed' })],
      extra: [flag('hasFailed', false)],
    },
  ),
  block('type.new', 'types', 'reporter', 'new %TYPE with %FIELD …', {
    output: 'S',
    fields: [field('TYPE', 'type', { type: { kind: 'var', name: 'S' } })],
    inputs: [
      input('FIELD', {
        optional: true,
        repeat: { count: 'fieldCount', plus: 0 },
        type: { kind: 'member', field: 'TYPE' },
      }),
    ],
    extra: [count('fieldCount', 0, 64, 0)],
    typeVars: { S: 'struct' },
    order: [{ field: 'TYPE' }, { input: 'FIELD' }],
  }),
  block(
    'type.struct',
    'types',
    'definition',
    'struct %NAME has … comparable %COMPARABLE printable %PRINTABLE share with other modules %SHARED',
    {
      fields: [
        field('NAME', 'symbol_decl'),
        field('COMPARABLE', 'checkbox', { default: false }),
        field('PRINTABLE', 'checkbox', { default: false }),
        field('SHARED', 'checkbox', { default: false }),
      ],
      extra: [
        {
          name: 'fields',
          kind: 'fields',
          min: null,
          max: 64,
          default: null,
          types: M2_TYPES,
          typeClass: 'value',
        },
      ],
    },
  ),
  block(
    'type.enum',
    'types',
    'definition',
    'enum %NAME values … share with other modules %SHARED',
    {
      fields: [field('NAME', 'symbol_decl'), field('SHARED', 'checkbox', { default: false })],
      extra: [
        {
          name: 'values',
          kind: 'values',
          min: null,
          max: 64,
          default: null,
          types: [],
          typeClass: null,
        },
      ],
    },
  ),
];

/** A W2-shaped definition by ID. */
export function w2Def(id: string): BlockDefJson {
  const def = W2_DEFS.find((candidate) => candidate.id === id);
  if (def === undefined) {
    throw new Error(`no W2 fixture ${id}`);
  }
  return def;
}
