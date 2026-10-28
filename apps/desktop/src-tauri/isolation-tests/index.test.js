// Tests of the isolation hook as the isolation frame runs it (docs/spec/08-security.md §8.8), with
// plain Node and one development dependency, happy-dom, which parses index.html:
//
//   node --test "apps/desktop/src-tauri/isolation-tests/*.test.js"
//
// index.html's three classic scripts are loaded into one fresh realm in the order the page lists
// them, with `window` as that realm's global object, the way Tauri inlines them into the frame.
// Messages are built inside the realm too, because structured cloning (postMessage) creates the
// frame's own objects.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

import { Window } from 'happy-dom';

const here = dirname(fileURLToPath(import.meta.url));
const isolation = join(here, '..', 'isolation');
const samples = JSON.parse(readFileSync(join(here, 'samples.generated.json'), 'utf8'));
const FETCH = 'plugin:__TAURI_CHANNEL__|fetch';

/**
 * The scripts `html` loads, in its order, read with an HTML parser (never a regular expression).
 * Every script must be a classic script with a plain file name in the isolation folder: anything
 * else (inline code, a module, a path) fails here instead of being left out of the tests.
 * @param {string} html
 */
function scriptsOf(html) {
  const window = new Window();
  try {
    const page = new window.DOMParser().parseFromString(html, 'text/html');
    const scripts = [...page.querySelectorAll('script')].map((script) => {
      const src = script.getAttribute('src');
      assert.ok(
        src !== null && /^[a-z][a-z.]*\.js$/.test(src),
        `unexpected script src ${JSON.stringify(src)}`,
      );
      assert.equal(script.textContent, '', `script ${src} also has inline code`);
      assert.ok(!script.hasAttribute('type'), `script ${src} has a type`);
      return src;
    });
    assert.ok(scripts.length > 0, 'the page loads no scripts');
    return scripts;
  } finally {
    void window.happyDOM.close();
  }
}

/** The scripts index.html loads, in its order. */
function pageScripts() {
  return scriptsOf(readFileSync(join(isolation, 'index.html'), 'utf8'));
}

/** A realm with the page's scripts loaded; returns the hook and a message builder. */
function loadFrame() {
  const realm = vm.createContext({});
  vm.runInContext('var window = globalThis;', realm);
  for (const file of pageScripts()) {
    vm.runInContext(readFileSync(join(isolation, file), 'utf8'), realm, { filename: file });
  }
  vm.runInContext(
    `function b2cTestMessage(cmd, payloadJson, optionsJson) {
       return {
         cmd,
         callback: 4242,
         error: 4243,
         payload: payloadJson === undefined ? undefined : JSON.parse(payloadJson),
         options: optionsJson === undefined ? undefined : JSON.parse(optionsJson),
       };
     }`,
    realm,
  );
  return vm.runInContext(
    `({
       hook: window.__TAURI_ISOLATION_HOOK__,
       makeMessage: b2cTestMessage,
       allowlist: B2C_IPC_ALLOWLIST,
       globals: { allowlist: typeof B2C_IPC_ALLOWLIST, validator: typeof b2cValidateIpcMessage },
     })`,
    realm,
  );
}

const frame = loadFrame();

/**
 * A message built inside the frame's realm.
 * @param {string} cmd
 * @param {unknown} payload - plain JSON data
 * @param {unknown} [options]
 */
function message(cmd, payload, options) {
  return frame.makeMessage(
    cmd,
    payload === undefined ? undefined : JSON.stringify(payload),
    options === undefined ? undefined : JSON.stringify(options),
  );
}

/** A deep copy of a command's sample payload. @param {string} cmd */
function sample(cmd) {
  return JSON.parse(JSON.stringify(samples[cmd]));
}

/** Asserts that the hook drops `msg`, with the given command shown in the error. */
function assertBlocked(msg, shown) {
  assert.throws(
    () => frame.hook(msg),
    (error) => {
      assert.match(error.message, /^Blocked an IPC message that is not on the allowlist/);
      if (shown !== undefined) {
        assert.ok(error.message.includes(`(command: ${shown})`), error.message);
      }
      return true;
    },
  );
}

describe('the isolation page', () => {
  test('reads the page with an HTML parser, whatever the spelling of its tags', () => {
    assert.deepEqual(scriptsOf('<SCRIPT SRC="a.js"></SCRIPT ><script\nsrc=b.js></script\t>'), [
      'a.js',
      'b.js',
    ]);
    assert.throws(() => scriptsOf('<script src="a.js"></script><script>alert(1)</script >'));
    assert.throws(() => scriptsOf('<script src="a.js">alert(1)</script>'));
    assert.throws(() => scriptsOf('<script type="module" src="a.js"></script>'));
    assert.throws(() => scriptsOf('<script src="../a.js"></script>'));
    assert.throws(() => scriptsOf('<p>no scripts</p>'));
  });

  test('loads the allowlist, the validator and the hook, in that order', () => {
    assert.deepEqual(pageScripts(), ['allowlist.generated.js', 'validate.js', 'index.js']);
  });

  test('installs the hook as a function', () => {
    assert.equal(typeof frame.hook, 'function');
    assert.deepEqual({ ...frame.globals }, { allowlist: 'object', validator: 'function' });
  });
});

describe('the hook', () => {
  test('passes every command with its sample payload, unchanged', () => {
    assert.equal(Object.keys(samples).length, 39);
    for (const [cmd, payload] of Object.entries(samples)) {
      const msg = message(cmd, payload);
      assert.equal(frame.hook(msg), msg, cmd);
    }
  });

  test("passes Tauri's channel fetch, which delivers large channel messages", () => {
    const fetch = message(FETCH, null, { headers: { 'Tauri-Channel-Id': '12' } });
    assert.equal(frame.hook(fetch), fetch);
    assertBlocked(message(FETCH, {}, { headers: { 'Tauri-Channel-Id': '12' } }), FETCH);
    assertBlocked(message(FETCH, null, { headers: { 'Tauri-Channel-Id': '1; drop' } }), FETCH);
  });

  test('blocks unknown commands, including the M0 command and plugin commands', () => {
    for (const cmd of [
      'app_version',
      'project_export_dialog',
      'plugin:dialog|open',
      'plugin:dialog|message',
      'plugin:event|listen',
      'plugin:fs|read_file',
    ]) {
      assertBlocked(message(cmd, {}), cmd);
    }
  });

  test('blocks an extra top-level key, which Tauri itself would ignore', () => {
    for (const cmd of ['app_info', 'project_save', 'build_start', 'run_start']) {
      const payload = sample(cmd);
      payload.request2 = {};
      assertBlocked(message(cmd, payload), cmd);
    }
  });

  test('blocks wrong types, oversized values and malformed IDs', () => {
    const save = sample('project_save');
    save.request.document = 7;
    assertBlocked(message('project_save', save));

    const big = sample('project_save');
    big.request.document = 'x'.repeat(33_554_433);
    assertBlocked(message('project_save', big));

    const input = sample('run_input');
    input.request.data = 'A'.repeat(87_388);
    assertBlocked(message('run_input', input));

    const forged = sample('trust_grant');
    forged.request.handle = 'ph_../../../etc/passwd';
    assertBlocked(message('trust_grant', forged));

    const resize = sample('run_resize');
    resize.request.cols = 100_000;
    assertBlocked(message('run_resize', resize));
  });

  test('blocks prototype-polluting keys at any depth', () => {
    for (const key of ['__proto__', 'constructor', 'prototype']) {
      const payload = sample('project_new');
      // JSON.parse creates an own property even for `__proto__`.
      const polluted = JSON.parse(
        JSON.stringify(payload).replace('"template"', `"${key}":{},"template"`),
      );
      assertBlocked(message('project_new', polluted));
    }
  });

  test('blocks anything that is not a message', () => {
    for (const value of [null, undefined, 'app_info', 42, []]) {
      assertBlocked(value, undefined);
    }
  });

  test('shows only plain command names in its error, never other text', () => {
    assertBlocked(message('<img src=x onerror=alert(1)>', {}), '(not shown)');
    assertBlocked(message('a'.repeat(65), {}), '(not shown)');
    assertBlocked(message('app_info\nfake log line', {}), '(not shown)');
  });
});

// The Windows nightly once saw `app_quit` run during the malformed-message cases
// (e2e/specs/security/README.md, "The app_quit incident"). Every command without arguments is one
// valid message, `{}`, away from running, so each must refuse every other payload.
describe('commands that take no arguments', () => {
  // Read from the generated allowlist, so a new command without arguments is covered at once.
  const argumentFree = Object.keys(frame.allowlist)
    .filter((cmd) => Object.keys(frame.allowlist[cmd]).length === 0)
    .sort();

  test('are the commands whose valid sample is empty, app_quit among them', () => {
    assert.ok(argumentFree.includes('app_quit'));
    assert.deepEqual(
      argumentFree,
      Object.keys(samples)
        .filter((cmd) => Object.keys(samples[cmd]).length === 0)
        .sort(),
    );
  });

  test('pass with an empty payload', () => {
    for (const cmd of argumentFree) {
      const msg = message(cmd, {});
      assert.equal(frame.hook(msg), msg, cmd);
    }
  });

  test('block a constructor, prototype or __proto__ key, an extra key and any other payload', () => {
    // JSON.parse makes each key an own property, `__proto__` included, as structured cloning
    // into the frame does.
    const payloads = [
      JSON.parse('{"constructor":{"prototype":{"b2cPolluted":true}}}'),
      JSON.parse('{"constructor":{}}'),
      JSON.parse('{"constructor":null}'),
      JSON.parse('{"prototype":{"b2cPolluted":true}}'),
      JSON.parse('{"prototype":1}'),
      JSON.parse('{"__proto__":{"b2cPolluted":true}}'),
      JSON.parse('{"__proto__":{}}'),
      { b2cExtra: 1 },
      { request: {} },
      { onEvent: '__CHANNEL__:1' },
      undefined,
      null,
      [],
      '{}',
      0,
    ];
    for (const cmd of argumentFree) {
      for (const payload of payloads) {
        assert.throws(
          () => frame.hook(message(cmd, payload)),
          (error) => error.message.endsWith(`(command: ${cmd})`),
          `${cmd} with ${JSON.stringify(payload)}`,
        );
      }
    }
  });

  test('block options, with or without those keys', () => {
    for (const cmd of argumentFree) {
      for (const options of [
        {},
        JSON.parse('{"constructor":1}'),
        JSON.parse('{"prototype":1}'),
        JSON.parse('{"__proto__":1}'),
        { headers: { 'Tauri-Channel-Id': '1' } },
      ]) {
        assertBlocked(message(cmd, {}, options), cmd);
      }
    }
  });
});
