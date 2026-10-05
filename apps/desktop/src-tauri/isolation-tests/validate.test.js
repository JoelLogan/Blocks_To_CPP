// Tests of the isolation hook's message validator (docs/spec/08-security.md §8.8), with plain
// Node and no dependencies:
//
//   node --test "apps/desktop/src-tauri/isolation-tests/*.test.js"
//
// The allowlist and the validator are classic scripts, so they are loaded into a fresh realm the
// way the isolation frame loads them. Messages are built inside that realm too, because
// structured cloning (postMessage) creates the frame's own objects.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const here = dirname(fileURLToPath(import.meta.url));
const isolation = join(here, '..', 'isolation');
const samples = JSON.parse(readFileSync(join(here, 'samples.generated.json'), 'utf8'));

const MAX_DOCUMENT = 33_554_432;
const HANDLE = 'ph_0123456789abcdef0123456789abcdef';
const RUN_ID = 'rn_0123456789abcdef0123456789abcdef';
const FETCH = 'plugin:__TAURI_CHANNEL__|fetch';

const realm = vm.createContext({});
for (const file of ['allowlist.generated.js', 'validate.js']) {
  vm.runInContext(readFileSync(join(isolation, file), 'utf8'), realm, { filename: file });
}
vm.runInContext(
  `function b2cTestMessage(cmd, payloadJson, optionsJson) {
     return {
       cmd,
       callback: 3988421,
       error: 3988422,
       payload: payloadJson === undefined ? undefined : JSON.parse(payloadJson),
       options: optionsJson === undefined ? undefined : JSON.parse(optionsJson),
     };
   }`,
  realm,
);
const { allowlist, validate, makeMessage } = vm.runInContext(
  '({ allowlist: B2C_IPC_ALLOWLIST, validate: b2cValidateIpcMessage, makeMessage: b2cTestMessage })',
  realm,
);

/**
 * A message built inside the isolation realm.
 * @param {string} cmd
 * @param {unknown} payload - plain JSON data
 * @param {unknown} [options]
 */
function message(cmd, payload, options) {
  return makeMessage(
    cmd,
    JSON.stringify(payload),
    options === undefined ? undefined : JSON.stringify(options),
  );
}

/** A deep copy of a command's sample payload. @param {string} cmd */
function sample(cmd) {
  return JSON.parse(JSON.stringify(samples[cmd]));
}

/** @param {string} cmd @param {unknown} payload */
function accepts(cmd, payload) {
  return validate(message(cmd, payload), allowlist);
}

describe('the allowlist', () => {
  test('covers exactly the commands with samples, and is frozen at every level', () => {
    assert.deepEqual(Object.keys(allowlist).sort(), Object.keys(samples).sort());
    assert.equal(Object.keys(allowlist).length, 36);
    const visit = (value) => {
      if (typeof value === 'object' && value !== null) {
        assert.ok(Object.isFrozen(value));
        Object.values(value).forEach(visit);
      }
    };
    visit(allowlist);
  });

  test('accepts the sample payload of every command', () => {
    for (const [cmd, payload] of Object.entries(samples)) {
      assert.ok(accepts(cmd, payload), cmd);
    }
  });
});

describe('top-level keys', () => {
  test('rejects an extra top-level key', () => {
    for (const cmd of ['app_info', 'project_save', 'build_start', 'run_start']) {
      const payload = sample(cmd);
      payload.extra = 1;
      assert.equal(accepts(cmd, payload), false, cmd);
    }
  });

  test('rejects a missing request or channel', () => {
    assert.equal(accepts('project_save', {}), false);
    const build = sample('build_start');
    delete build.onEvent;
    assert.equal(accepts('build_start', build), false);
    const run = sample('run_start');
    delete run.onOutput;
    assert.equal(accepts('run_start', run), false);
  });

  test('accepts only channel strings in channel arguments', () => {
    for (const channel of ['__CHANNEL__:0', '__CHANNEL__:4294967295']) {
      assert.ok(accepts('app_subscribe', { onEvent: channel }), channel);
    }
    for (const channel of [
      '__CHANNEL__:',
      '__CHANNEL__:12345678901',
      '__CHANNEL__:1a',
      ' __CHANNEL__:1',
      7,
      null,
      {},
    ]) {
      assert.equal(accepts('app_subscribe', { onEvent: channel }), false, String(channel));
    }
  });
});

describe('request fields', () => {
  test('rejects an extra nested key', () => {
    const save = sample('project_save');
    save.request.extra = true;
    assert.equal(accepts('project_save', save), false);
    const run = sample('run_start');
    run.request.runOptions.extra = 1;
    assert.equal(accepts('run_start', run), false);
  });

  test('rejects a missing required field', () => {
    const save = sample('project_save');
    delete save.request.document;
    assert.equal(accepts('project_save', save), false);
  });

  test('rejects wrong types', () => {
    const cases = [
      ['project_close', { request: { handle: 5 } }],
      ['project_set_dirty', { request: { handle: HANDLE, dirty: 'yes' } }],
      ['project_save', { request: { handle: HANDLE, document: { a: 1 } } }],
      ['run_resize', { request: { runId: RUN_ID, cols: '80', rows: 24 } }],
      ['run_resize', { request: { runId: RUN_ID, cols: 80.5, rows: 24 } }],
      ['run_start', { ...sample('run_start'), request: { buildId: HANDLE, runOptions: [80, 24] } }],
      ['project_new', { request: ['helloWorld'] }],
      ['project_new', { request: null }],
      ['project_new', []],
      ['project_new', 'request'],
      ['app_info', null],
    ];
    for (const [cmd, payload] of cases) {
      assert.equal(accepts(cmd, payload), false, `${cmd} ${JSON.stringify(payload)}`);
    }
  });

  test('checks document sizes before anything else', () => {
    const save = message('project_save', { request: { handle: HANDLE, document: '' } });
    save.payload.request.document = 'x'.repeat(MAX_DOCUMENT);
    assert.ok(validate(save, allowlist));
    save.payload.request.document = 'x'.repeat(MAX_DOCUMENT + 1);
    assert.equal(validate(save, allowlist), false);
  });

  test('rejects malformed IDs of every kind', () => {
    const ids = [
      ['project_close', 'handle', 'ph_'],
      ['recent_remove', 'recentId', 'rc_'],
      ['recovery_discard', 'snapshotId', 'sn_'],
      ['build_cancel', 'buildId', 'bd_'],
      ['run_stop', 'runId', 'rn_'],
    ];
    for (const [cmd, field, prefix] of ids) {
      const good = `${prefix}${'0123456789abcdef'.repeat(2)}`;
      assert.ok(accepts(cmd, { request: { [field]: good } }), cmd);
      for (const bad of [
        good.toUpperCase(),
        good.slice(0, -1),
        `${good}0`,
        `xx_${good.slice(3)}`,
        `${prefix}${'g'.repeat(32)}`,
        '',
      ]) {
        assert.equal(accepts(cmd, { request: { [field]: bad } }), false, `${cmd} ${bad}`);
      }
    }
    const toolchain = 'tc_0123456789abcdef';
    assert.ok(accepts('toolchain_select', { request: { toolchainId: toolchain } }));
    assert.equal(accepts('toolchain_select', { request: { toolchainId: `${toolchain}0` } }), false);
    assert.equal(accepts('toolchain_select', { request: { toolchainId: HANDLE } }), false);
  });

  test('rejects values outside enums and ranges', () => {
    const cases = [
      ['project_new', { request: { template: 'guessingGame' } }],
      ['open_help_link', { request: { linkId: 'https://example.com/' } }],
      [
        'build_start',
        { ...sample('build_start'), request: { ...sample('build_start').request, config: 'fast' } },
      ],
      ['run_resize', { request: { runId: RUN_ID, cols: 1, rows: 24 } }],
      ['run_resize', { request: { runId: RUN_ID, cols: 1001, rows: 24 } }],
      ['run_resize', { request: { runId: RUN_ID, cols: 80, rows: 0 } }],
      ['run_resize', { request: { runId: RUN_ID, cols: 80, rows: 1001 } }],
      ['run_ack', { request: { runId: RUN_ID, seq: -1 } }],
      ['run_ack', { request: { runId: RUN_ID, seq: Number.MAX_SAFE_INTEGER + 2 } }],
    ];
    for (const [cmd, payload] of cases) {
      assert.equal(accepts(cmd, payload), false, `${cmd} ${JSON.stringify(payload)}`);
    }
    assert.ok(accepts('run_resize', { request: { runId: RUN_ID, cols: 2, rows: 1 } }));
    assert.ok(accepts('run_resize', { request: { runId: RUN_ID, cols: 1000, rows: 1000 } }));
    assert.ok(accepts('run_ack', { request: { runId: RUN_ID, seq: Number.MAX_SAFE_INTEGER } }));
  });

  test('bounds run input', () => {
    const input = (data) => accepts('run_input', { request: { runId: RUN_ID, data } });
    assert.ok(input(''));
    assert.ok(input('NDIK'));
    assert.ok(input('A'.repeat(87_380) + 'AA=='));
    // 87,384 characters that decode to 65,537 bytes.
    assert.equal(input('A'.repeat(87_380) + 'AAA='), false);
    assert.equal(input('A'.repeat(87_388)), false);
    assert.equal(input('A'.repeat(87_385)), false);
    for (const bad of ['NDI', 'ND I', 'NDI=K', 'N===', 'NDIK\n', 'ND-_']) {
      assert.equal(input(bad), false, bad);
    }
  });

  test('settings_update takes only the editable keys', () => {
    assert.ok(accepts('settings_update', { request: {} }));
    assert.ok(accepts('settings_update', { request: { codeStyle: {} } }));
    assert.ok(accepts('settings_update', { request: { console: { scrollbackLines: 1000 } } }));
    for (const request of [
      { toolchain: { selectedId: null } },
      { newProject: { standard: 'c++23' } },
      { buildCache: { maxBytes: 1 } },
      { codeStyle: null },
      { codeStyle: { indentWidth: 3 } },
      { codeStyle: { indentWidth: null } },
      { run: { onErrors: 'ignore' } },
      { console: { scrollbackLines: 999 } },
      { console: { scrollbackLines: 100_001 } },
    ]) {
      assert.equal(accepts('settings_update', { request }), false, JSON.stringify(request));
    }
  });
});

describe('prototype pollution', () => {
  test('rejects __proto__, constructor and prototype keys at any depth', () => {
    for (const key of ['__proto__', 'constructor', 'prototype']) {
      const top = JSON.parse(`{"${key}": {}}`);
      assert.equal(accepts('app_info', top), false, `top ${key}`);
      const nested = sample('run_start');
      nested.request = { ...nested.request, ...JSON.parse(`{"${key}": 1}`) };
      assert.equal(accepts('run_start', nested), false, `request ${key}`);
      const deep = sample('run_start');
      deep.request.runOptions = { ...deep.request.runOptions, ...JSON.parse(`{"${key}": 1}`) };
      assert.equal(accepts('run_start', deep), false, `runOptions ${key}`);
      assert.equal(
        validate(makeMessage('app_info', '{}', JSON.stringify({ [key]: 1 })), allowlist),
        false,
      );
    }
  });

  test('rejects objects from another realm and non-plain objects', () => {
    const outside = { cmd: 'app_info', callback: 1, error: 2, payload: {}, options: undefined };
    assert.equal(validate(outside, allowlist), false);
    const inside = message('project_new', { request: { template: 'empty' } });
    inside.payload.request = { template: 'empty' };
    assert.equal(validate(inside, allowlist), false);
  });
});

describe('commands and messages', () => {
  test('rejects unknown commands, including inherited property names', () => {
    for (const cmd of [
      'app_version',
      'project_export_dialog',
      'plugin:fs|read_file',
      'plugin:event|listen',
      '__proto__',
      'constructor',
      'toString',
      'hasOwnProperty',
      '',
    ]) {
      assert.equal(accepts(cmd, {}), false, cmd);
    }
  });

  test("accepts Tauri's channel fetch with a null payload", () => {
    assert.ok(validate(message(FETCH, null), allowlist));
    assert.ok(validate(message(FETCH, null, { headers: { 'Tauri-Channel-Id': '42' } }), allowlist));
    assert.equal(validate(message(FETCH, {}), allowlist), false);
    assert.equal(validate(message(FETCH, null, { headers: {} }), allowlist), false);
    assert.equal(
      validate(message(FETCH, null, { headers: { 'Tauri-Channel-Id': 'x' } }), allowlist),
      false,
    );
    assert.equal(
      validate(
        message(FETCH, null, { headers: { 'Tauri-Channel-Id': '1', Cookie: 'a' } }),
        allowlist,
      ),
      false,
    );
    assert.equal(validate(message(FETCH, null, { method: 'POST' }), allowlist), false);
  });

  test('checks the message envelope', () => {
    const good = message('app_info', {});
    assert.ok(validate(good, allowlist));
    for (const change of [
      (m) => (m.extra = 1),
      (m) => (m.callback = 'x'),
      (m) => (m.error = -1),
      (m) => (m.callback = 2 ** 32),
      (m) => delete m.callback,
      (m) => (m.cmd = 5),
    ]) {
      const copy = message('app_info', {});
      change(copy);
      assert.equal(validate(copy, allowlist), false, String(change));
    }
    const withOptions = message('app_info', {}, { headers: { 'Tauri-Channel-Id': '1' } });
    assert.equal(validate(withOptions, allowlist), false);
  });

  test('never throws on odd input', () => {
    for (const odd of [undefined, null, 0, 'app_info', [], () => 1]) {
      assert.equal(validate(odd, allowlist), false);
    }
    assert.equal(validate(message('app_info', {}), null), false);
    assert.equal(validate(message('app_info', {}), {}), false);
  });
});
