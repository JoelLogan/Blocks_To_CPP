/**
 * The IPC abuse cases of the nightly security tests (docs/spec/08-security.md §8.8 and §8.13,
 * items 3 and 4; threat T8 of §8.12), built from the valid sample of every command that
 * `crates/b2c-ipc` generates for the isolation hook's own tests
 * (src-tauri/isolation-tests/samples.generated.json). A new command therefore gets its abuse
 * cases without a change here.
 *
 * Each case says what must become of it: dropped by the isolation hook (never sent), or refused
 * by the backend with a typed error. The limits are those of the isolation allowlist and the
 * backend (02 §2.5, 08 §8.8): documents up to 33,554,432 UTF-16 units, program input up to
 * 65,536 bytes (87,384 base64 characters), terminals of 2–1000 columns and 1–1000 rows, and
 * IDs of their prefix and exactly 32 (toolchains: 16) lower-case hex digits.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { REPOSITORY_ROOT } from '../../../support/env';
import type { AbuseCase, Expected } from './ipc';

/** The generated valid sample of every command's arguments. */
export const SAMPLES_FILE = path.join(
  REPOSITORY_ROOT,
  'apps',
  'desktop',
  'src-tauri',
  'isolation-tests',
  'samples.generated.json',
);

/** A command's sample arguments: a JSON object. */
export type Sample = Readonly<Record<string, unknown>>;

/** The samples by command name. */
export type Samples = Readonly<Record<string, Sample>>;

/** Reads the samples, checking that each is a JSON object. */
export function readSamples(file = SAMPLES_FILE): Samples {
  const value: unknown = JSON.parse(readFileSync(file, 'utf8'));
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new Error(`${file} does not hold an object`);
  }
  for (const [command, sample] of Object.entries(value)) {
    if (typeof sample !== 'object' || sample === null || Array.isArray(sample)) {
      throw new Error(`The sample of ${command} is not an object`);
    }
  }
  return value as Samples;
}

/** The largest document the hook sends, in UTF-16 units (02 §2.5). */
export const MAX_DOCUMENT_UNITS = 33_554_432;

/** The most program input per `run_input`, in bytes (08 §8.8). */
export const MAX_INPUT_BYTES = 65_536;

/** A well-formed ID of each kind that names nothing. */
export const FORGED = {
  handle: 'ph_00000000000000000000000000000000',
  build: 'bd_00000000000000000000000000000000',
  run: 'rn_00000000000000000000000000000000',
  recent: 'rc_00000000000000000000000000000000',
  snapshot: 'sn_00000000000000000000000000000000',
  toolchain: 'tc_0000000000000000',
} as const;

/** IDs in a shape the hook refuses, for a field that takes an ID with `prefix`. */
export function malformedIds(prefix: string, hexLength: number): string[] {
  const hex = '0'.repeat(hexLength);
  return [
    `${prefix}../../../../etc/passwd`,
    `${prefix}${hex.slice(1)}`,
    `${prefix}${hex}0`,
    `${prefix}${'A'.repeat(hexLength)}`,
    `${prefix}${hex.slice(1)}\u0000`,
    `${prefix.toUpperCase()}${hex}`,
    hex,
    path.join(path.sep, 'etc', 'passwd'),
    '',
  ];
}

/** The base64 text of `count` zero bytes (as a program's input). */
export function zeroBytesBase64(count: number): string {
  const whole = Math.floor(count / 3);
  const rest = count % 3;
  return 'A'.repeat(whole * 4) + (rest === 1 ? 'AA==' : rest === 2 ? 'AAA=' : '');
}

/** A deep copy of plain JSON data. */
function copy<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/** Whether a sample argument is a channel (`__CHANNEL__:<n>`). */
function isChannel(value: unknown): boolean {
  return typeof value === 'string' && value.startsWith('__CHANNEL__:');
}

/** A value of another JSON type than `value`, of the same "size". */
function otherType(value: unknown): unknown {
  switch (typeof value) {
    case 'string':
      return 7;
    case 'number':
      return String(value);
    case 'boolean':
      return value ? 'true' : 'false';
    default:
      return Array.isArray(value) ? {} : [value];
  }
}

/** JSON text of `object` with a leading `"__proto__"` key holding `{"b2cPolluted": true}`. */
export function withProtoKey(object: Readonly<Record<string, unknown>>): string {
  const rest = JSON.stringify(object).slice(1);
  return `{"__proto__":{"b2cPolluted":true}${rest === '}' ? '' : ','}${rest}`;
}

const DROPPED: Expected = 'dropped';

/**
 * Every command's sample turned into messages the isolation hook must drop: an extra top-level
 * key, a `constructor` or `prototype` key at the top and in the request, a missing or wrongly
 * typed `request`, an unknown or wrongly typed request field, and a channel argument that is not
 * a channel. (`__proto__` keys never reach the hook: see {@link protoCases}.)
 */
export function hookMutations(samples: Samples): AbuseCase[] {
  const cases: AbuseCase[] = [];
  const add = (name: string, cmd: string, args: unknown) => {
    cases.push({ name, cmd, args, expected: DROPPED });
  };
  for (const [cmd, sample] of Object.entries(samples)) {
    add(`${cmd}: an extra argument`, cmd, { ...copy(sample), b2cExtra: 1 });
    add(`${cmd}: a constructor argument`, cmd, {
      ...copy(sample),
      constructor: { prototype: { b2cPolluted: true } },
    });
    add(`${cmd}: a prototype argument`, cmd, { ...copy(sample), prototype: { b2cPolluted: true } });
    const request = sample['request'];
    if (typeof request === 'object' && request !== null && !Array.isArray(request)) {
      const fields = request as Readonly<Record<string, unknown>>;
      const rest = Object.fromEntries(Object.entries(sample).filter(([key]) => key !== 'request'));
      add(`${cmd}: no request`, cmd, copy(rest));
      add(`${cmd}: a null request`, cmd, { ...copy(sample), request: null });
      add(`${cmd}: an array request`, cmd, { ...copy(sample), request: [copy(fields)] });
      add(`${cmd}: a string request`, cmd, { ...copy(sample), request: JSON.stringify(fields) });
      add(`${cmd}: an unknown request field`, cmd, {
        ...copy(sample),
        request: { ...copy(fields), b2cExtra: 'x' },
      });
      add(`${cmd}: a constructor request field`, cmd, {
        ...copy(sample),
        request: { ...copy(fields), constructor: { prototype: { b2cPolluted: true } } },
      });
      add(`${cmd}: a prototype request field`, cmd, {
        ...copy(sample),
        request: { ...copy(fields), prototype: { b2cPolluted: true } },
      });
      for (const [field, value] of Object.entries(fields)) {
        add(`${cmd}: ${field} of another type`, cmd, {
          ...copy(sample),
          request: { ...copy(fields), [field]: otherType(value) },
        });
      }
    }
    for (const [key, value] of Object.entries(sample)) {
      if (isChannel(value)) {
        add(`${cmd}: ${key} not a channel`, cmd, { ...copy(sample), [key]: 'not a channel' });
        add(`${cmd}: ${key} a forged channel`, cmd, { ...copy(sample), [key]: '__CHANNEL__:x1' });
      }
    }
  }
  return cases;
}

/**
 * `__proto__` keys in `invoke` arguments. Tauri's serializer in the editor's frame copies the
 * arguments with `copy[key] = value`, which turns a `__proto__` key into the copy's prototype, and
 * structured cloning into the isolation frame keeps own properties only: the key reaches neither
 * the hook nor the backend. What must hold is that nothing travels through it: a field hidden
 * behind `__proto__` does not arrive (the message is then incomplete and dropped), a `__proto__`
 * key beside a field leaves the field alone (refused for the forged handle it names), and no
 * prototype of the page changes (the test checks that). A command without side effects is used,
 * because what arrives may be a valid message.
 */
export function protoCases(): AbuseCase[] {
  const behind = (value: unknown) => `{"__proto__":${JSON.stringify(value)}}`;
  return [
    {
      name: 'a request field behind __proto__',
      cmd: 'trust_get',
      args: { request: { $json: behind({ handle: FORGED.handle }) } },
      expected: DROPPED,
    },
    {
      name: 'a __proto__ key beside the request field',
      cmd: 'trust_get',
      args: { request: { $json: withProtoKey({ handle: FORGED.handle }) } },
      expected: { code: 'unknownHandle' },
    },
    {
      name: 'the arguments behind __proto__',
      cmd: 'trust_get',
      args: { $json: behind({ request: { handle: FORGED.handle } }) },
      expected: DROPPED,
    },
    {
      name: 'a __proto__ key beside the arguments',
      cmd: 'trust_get',
      args: { $json: withProtoKey({ request: { handle: FORGED.handle } }) },
      expected: { code: 'unknownHandle' },
    },
  ];
}

/** A case with a name, built from a command's sample with `request` fields replaced. */
function withRequest(
  samples: Samples,
  name: string,
  cmd: string,
  fields: Readonly<Record<string, unknown>>,
  expected: Expected,
): AbuseCase {
  const sample = samples[cmd];
  if (sample === undefined) {
    throw new Error(`No sample for ${cmd}`);
  }
  const request = sample['request'];
  if (typeof request !== 'object' || request === null) {
    throw new Error(`The sample of ${cmd} has no request`);
  }
  return {
    name,
    cmd,
    args: { ...copy(sample), request: { ...copy(request), ...fields } },
    expected,
  };
}

/**
 * The limits at their edges: the largest valid value passes the hook and the backend refuses it
 * for the forged ID it names; one more is dropped. Oversized documents and program input,
 * terminal sizes, acknowledgement numbers.
 */
export function limitCases(samples: Samples): AbuseCase[] {
  const unknownRun: Expected = { code: 'unknownRun' };
  const unknownBuild: Expected = { code: 'unknownBuild' };
  const cases: AbuseCase[] = [
    withRequest(
      samples,
      'document of 33,554,433 units',
      'project_save',
      {
        handle: FORGED.handle,
        document: { $repeat: { text: ' ', count: MAX_DOCUMENT_UNITS + 1 } },
      },
      DROPPED,
    ),
    withRequest(
      samples,
      'recovery document of 33,554,433 units',
      'recovery_save',
      {
        handle: FORGED.handle,
        document: { $repeat: { text: ' ', count: MAX_DOCUMENT_UNITS + 1 } },
      },
      DROPPED,
    ),
    withRequest(
      samples,
      'build document of 33,554,433 units',
      'build_start',
      {
        handle: FORGED.handle,
        document: { $repeat: { text: ' ', count: MAX_DOCUMENT_UNITS + 1 } },
      },
      DROPPED,
    ),
    withRequest(
      samples,
      'input of 65,536 bytes',
      'run_input',
      {
        runId: FORGED.run,
        data: zeroBytesBase64(MAX_INPUT_BYTES),
      },
      unknownRun,
    ),
    withRequest(
      samples,
      'input of 65,537 bytes',
      'run_input',
      {
        runId: FORGED.run,
        data: zeroBytesBase64(MAX_INPUT_BYTES + 1),
      },
      DROPPED,
    ),
    withRequest(
      samples,
      'input of 87,388 base64 characters',
      'run_input',
      {
        runId: FORGED.run,
        data: 'A'.repeat(87_388),
      },
      DROPPED,
    ),
    withRequest(
      samples,
      'input that is not base64',
      'run_input',
      {
        runId: FORGED.run,
        data: '<script>!</script>',
      },
      DROPPED,
    ),
    withRequest(
      samples,
      'input of a broken base64 length',
      'run_input',
      {
        runId: FORGED.run,
        data: 'AAA',
      },
      DROPPED,
    ),
    withRequest(
      samples,
      'run_ack of the largest batch number',
      'run_ack',
      {
        runId: FORGED.run,
        seq: Number.MAX_SAFE_INTEGER,
      },
      unknownRun,
    ),
    withRequest(
      samples,
      'run_ack of a negative batch',
      'run_ack',
      {
        runId: FORGED.run,
        seq: -1,
      },
      DROPPED,
    ),
    withRequest(
      samples,
      'run_ack beyond the safe integers',
      'run_ack',
      {
        runId: FORGED.run,
        seq: 2 ** 53,
      },
      DROPPED,
    ),
  ];
  const sizes: [string, number, number, boolean][] = [
    ['2 × 1', 2, 1, true],
    ['1000 × 1000', 1000, 1000, true],
    ['1 column', 1, 24, false],
    ['1001 columns', 1001, 24, false],
    ['0 rows', 80, 0, false],
    ['1001 rows', 80, 1001, false],
    ['negative columns', -80, 24, false],
    ['80.5 columns', 80.5, 24, false],
    ['2^53 columns', 2 ** 53, 24, false],
  ];
  for (const [label, cols, rows, valid] of sizes) {
    cases.push(
      withRequest(
        samples,
        `resize to ${label}`,
        'run_resize',
        { runId: FORGED.run, cols, rows },
        valid ? unknownRun : DROPPED,
      ),
      withRequest(
        samples,
        `run in a terminal of ${label}`,
        'run_start',
        {
          buildId: FORGED.build,
          runOptions: { cols, rows },
        },
        valid ? unknownBuild : DROPPED,
      ),
    );
  }
  cases.push(
    withRequest(
      samples,
      'resize to "80" columns',
      'run_resize',
      {
        runId: FORGED.run,
        cols: '80',
        rows: 24,
      },
      DROPPED,
    ),
  );
  return cases;
}

/**
 * Forged IDs: malformed ones are dropped by the hook; well-formed ones that name nothing are
 * refused by the backend with the matching `unknown…` error. No ID is a path, and a path is no ID.
 */
export function forgedIdCases(samples: Samples): AbuseCase[] {
  const cases: AbuseCase[] = [];
  const kinds: [string, string, string, number, string, string][] = [
    // command, field, prefix, hex digits, forged ID, error code
    ['project_close', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['project_reload', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['project_save', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['project_save_as_dialog', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['project_set_dirty', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['recovery_save', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['trust_get', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['trust_grant', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['trust_revoke', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['build_start', 'handle', 'ph_', 32, FORGED.handle, 'unknownHandle'],
    ['build_cancel', 'buildId', 'bd_', 32, FORGED.build, 'unknownBuild'],
    ['run_start', 'buildId', 'bd_', 32, FORGED.build, 'unknownBuild'],
    ['run_input', 'runId', 'rn_', 32, FORGED.run, 'unknownRun'],
    ['run_resize', 'runId', 'rn_', 32, FORGED.run, 'unknownRun'],
    ['run_stop', 'runId', 'rn_', 32, FORGED.run, 'unknownRun'],
    ['run_ack', 'runId', 'rn_', 32, FORGED.run, 'unknownRun'],
    ['project_open_recent', 'recentId', 'rc_', 32, FORGED.recent, 'unknownRecent'],
    ['recent_remove', 'recentId', 'rc_', 32, FORGED.recent, 'unknownRecent'],
    ['recovery_restore', 'snapshotId', 'sn_', 32, FORGED.snapshot, 'unknownSnapshot'],
    ['recovery_discard', 'snapshotId', 'sn_', 32, FORGED.snapshot, 'unknownSnapshot'],
    ['toolchain_select', 'toolchainId', 'tc_', 16, FORGED.toolchain, 'unknownToolchain'],
  ];
  for (const [cmd, field, prefix, hexLength, forged, code] of kinds) {
    cases.push(
      withRequest(samples, `${cmd}: a forged ${field}`, cmd, { [field]: forged }, { code }),
    );
    malformedIds(prefix, hexLength).forEach((id, index) => {
      cases.push(
        withRequest(
          samples,
          `${cmd}: malformed ${field} #${String(index + 1)}`,
          cmd,
          { [field]: id },
          DROPPED,
        ),
      );
    });
  }
  return cases;
}

/**
 * Documents the hook passes (it checks only their length) and the backend's strict loader refuses
 * (05 §5.6): not JSON, another format, unknown keys, a `__proto__` key, compiler flags, duplicate
 * keys, a newer format. The handle is forged: the document is checked first.
 */
export function documentCases(samples: Samples): AbuseCase[] {
  const sample = samples['project_save'];
  const request = sample?.['request'];
  const valid =
    typeof request === 'object' && request !== null
      ? (request as { readonly document?: unknown }).document
      : undefined;
  if (typeof valid !== 'string') {
    throw new Error('The sample of project_save has no document');
  }
  const document = JSON.parse(valid) as Record<string, unknown>;
  const project = document['project'] as Record<string, unknown>;
  const invalid: Expected = { code: 'invalidDocument' };
  const documents: [string, string, Expected][] = [
    ['valid (forged handle)', valid, { code: 'unknownHandle' }],
    ['not JSON', '<script>window.__b2cXss = 1</script>', invalid],
    ['another JSON file', '{"name": "package", "version": "1.0.0"}', invalid],
    ['an unknown top-level key', JSON.stringify({ ...document, evil: true }), invalid],
    ['a __proto__ key', withProtoKey(document), invalid],
    [
      'compiler flags',
      JSON.stringify({
        ...document,
        project: { ...project, build: { flags: ['-fplugin=/tmp/evil.so'] } },
      }),
      invalid,
    ],
    ['a duplicate key', `{"format":"x",${valid.trim().slice(1)}`, invalid],
    ['a newer format', JSON.stringify({ ...document, formatVersion: 99 }), { code: 'newerFormat' }],
  ];
  return documents.flatMap(([label, text, expected]) =>
    (['project_save', 'recovery_save', 'build_start'] as const).map((cmd) =>
      withRequest(
        samples,
        `${cmd}: ${label}`,
        cmd,
        { handle: FORGED.handle, document: text },
        expected,
      ),
    ),
  );
}

/**
 * Commands that are not ours (Tauri's core and plugin commands, which the capability does not
 * grant, and names that only look like ours): the hook drops them.
 */
export const FOREIGN_COMMANDS = [
  'b2c_e2e_no_such_command',
  'APP_INFO',
  'app_info ',
  'app_info\u0000',
  'plugin:b2c|app_info',
  'plugin:__TAURI_CHANNEL__|fetch2',
  'plugin:app|version',
  'plugin:app|app_show',
  'plugin:event|listen',
  'plugin:event|emit',
  'plugin:window|close',
  'plugin:window|set_title',
  'plugin:webview|create_webview_window',
  'plugin:webview|internal_toggle_devtools',
  'plugin:path|resolve_directory',
  'plugin:resources|close',
  'plugin:menu|new',
  'plugin:dialog|open',
  'plugin:dialog|message',
  'plugin:opener|open_url',
  'plugin:shell|execute',
  'plugin:fs|read_file',
] as const;

/**
 * Messages sent around the isolation frame: with a plain JSON body (the backend expects an
 * encrypted one and refuses it) for a sample of commands, and with an empty body (which Tauri
 * does not decrypt) for every command that takes arguments and every foreign command. None may
 * be answered.
 */
export function bypassCases(samples: Samples): AbuseCase[] {
  const cases: AbuseCase[] = [];
  for (const [cmd, sample] of Object.entries(samples)) {
    cases.push({
      name: `${cmd}: plain JSON around the hook`,
      cmd,
      args: copy(sample),
      route: 'aroundHook',
      expected: 'notAccepted',
    });
    if (Object.keys(sample).length > 0) {
      cases.push({
        name: `${cmd}: empty body around the hook`,
        cmd,
        route: 'aroundHookEmpty',
        expected: 'notAccepted',
      });
    }
  }
  for (const cmd of FOREIGN_COMMANDS) {
    cases.push({
      name: `${cmd}: empty body around the hook`,
      cmd,
      route: 'aroundHookEmpty',
      expected: 'notAccepted',
    });
  }
  return cases;
}
