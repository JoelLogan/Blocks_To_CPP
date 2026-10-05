#!/usr/bin/env node
// Builds @blocks2cpp/b2c-core-wasm: crates/b2c-core-wasm → WebAssembly → wasm-bindgen glue →
// wasm-opt → the embedded-bytes chunk (docs/spec/06-compiler-pipeline.md §6.13, ADR-0003).
//
//   pnpm --filter @blocks2cpp/b2c-core-wasm build
//
// Steps:
//   1. cargo build --locked --profile wasm-release --target wasm32-unknown-unknown -p b2c-core-wasm
//   2. wasm-bindgen --target web (the CLI must be the version of the wasm-bindgen crate in
//      Cargo.lock), and a check that the crate's exports are the ones src/glue.d.ts declares
//   3. wasm-opt -Oz with the WebAssembly features rustc emits (skipped with B2C_SKIP_WASM_OPT=1)
//   4. pkg/glue.js: the generated glue wrapped in createGlue(), one glue state per instance
//   5. pkg/pkg-bytes.js: the module as base64 (WASM_BASE64), for the lazily imported chunk
//   6. the sizes, printed and written to pkg/sizes.json; the build fails over the size budget
//
// Environment:
//   CARGO              cargo to run (default: cargo on PATH)
//   WASM_BINDGEN       the wasm-bindgen CLI (default: wasm-bindgen on PATH)
//   WASM_OPT           binaryen's wasm-opt (default: wasm-opt on PATH)
//   B2C_SKIP_WASM_OPT  1 skips wasm-opt (local builds where binaryen is missing or too old); the
//                      budget is then checked on the larger, unoptimised module
//
// Only Node built-ins; no shell: every tool runs with an argument list.

import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

/** The size budget for the optimised module (ADR-0003: at most 2 MB after wasm-opt -Oz). */
const SIZE_BUDGET_BYTES = 2_000_000;

const CRATE = 'b2c-core-wasm';
const STEM = 'b2c_core_wasm';

/**
 * The crate's exports as wasm-bindgen declares them. src/glue.d.ts, src/core.ts and the
 * wrapper below follow this list; the build stops when the crate's exports differ.
 */
const EXPECTED_EXPORTS = [
  'export function canonical(document_json: string): string;',
  'export function clipboard_make(document_json: string, block_ids_json: string): string;',
  'export function conversion_table(): string;',
  'export function load(bytes: Uint8Array): string;',
  'export function paste_prepare(clipboard_text: string, document_json: string, target_json: string, seed_hex: string): string;',
  'export function preview(document_json: string, options_json: string): string;',
  'export function symbols_in_scope(block_id: string, input?: string | null): string;',
  'export function version(): string;',
];
const EXPORTED_FUNCTIONS = [
  'canonical',
  'clipboard_make',
  'conversion_table',
  'load',
  'paste_prepare',
  'preview',
  'symbols_in_scope',
  'version',
];

/**
 * The WebAssembly features rustc 1.97 enables for wasm32-unknown-unknown (LLVM's generic CPU),
 * as wasm-opt flags. The module carries no target_features section (the release profile strips
 * it), so wasm-opt must be told. The optional ones are passed when this wasm-opt knows them.
 */
const REQUIRED_FEATURES = [
  '--enable-bulk-memory',
  '--enable-multivalue',
  '--enable-mutable-globals',
  '--enable-nontrapping-float-to-int',
  '--enable-reference-types',
  '--enable-sign-ext',
];
const OPTIONAL_FEATURES = ['--enable-bulk-memory-opt', '--enable-call-indirect-overlong'];

const packageDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(packageDir, '..', '..');
const pkgDir = path.join(packageDir, 'pkg');
const workDir = path.join(pkgDir, '.work');

class BuildError extends Error {}

function tool(variable, fallback) {
  const value = process.env[variable];
  return value === undefined || value === '' ? fallback : value;
}

/** Runs a tool and returns its standard output; its standard error goes to ours. */
function capture(command, args, options = {}) {
  try {
    return execFileSync(command, args, {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
      maxBuffer: 256 * 1024 * 1024,
      ...options,
    });
  } catch (error) {
    const reason = error.code === 'ENOENT' ? 'not found' : `failed (${error.message})`;
    throw new BuildError(`${command} ${args.join(' ')}: ${reason}`);
  }
}

/** The wasm-bindgen version pinned in Cargo.lock. */
function lockedWasmBindgenVersion() {
  const lock = readFileSync(path.join(repoRoot, 'Cargo.lock'), 'utf8');
  const versions = [
    ...lock.matchAll(/^\[\[package\]\]\nname = "wasm-bindgen"\nversion = "([^"]+)"$/gm),
  ].map((match) => match[1]);
  if (versions.length !== 1) {
    throw new BuildError(
      `Cargo.lock should hold exactly one wasm-bindgen version, found ${versions.length}`,
    );
  }
  return versions[0];
}

/** Step 1: the WebAssembly module, located through cargo's JSON messages. */
function cargoBuild() {
  const cargo = tool('CARGO', 'cargo');
  const output = capture(
    cargo,
    [
      'build',
      '--locked',
      '--profile',
      'wasm-release',
      '--target',
      'wasm32-unknown-unknown',
      '-p',
      CRATE,
      '--message-format=json-render-diagnostics',
    ],
    { cwd: repoRoot },
  );
  const artifacts = output
    .split('\n')
    .filter((line) => line.startsWith('{'))
    .map((line) => JSON.parse(line))
    .filter((message) => message.reason === 'compiler-artifact' && message.target?.name === STEM)
    .flatMap((message) => message.filenames)
    .filter((file) => file.endsWith('.wasm'));
  if (artifacts.length !== 1) {
    throw new BuildError(`cargo built ${artifacts.length} .wasm files for ${CRATE}, expected 1`);
  }
  return artifacts[0];
}

/** Step 2: wasm-bindgen, with the version and export checks. */
function bindgen(wasmFile, expectedVersion) {
  const wasmBindgen = tool('WASM_BINDGEN', 'wasm-bindgen');
  const version = capture(wasmBindgen, ['--version']).trim();
  if (version !== `wasm-bindgen ${expectedVersion}`) {
    throw new BuildError(
      `${wasmBindgen} is "${version}", but Cargo.lock pins wasm-bindgen ${expectedVersion}; ` +
        `install it with: cargo install wasm-bindgen-cli --version ${expectedVersion} --locked`,
    );
  }
  capture(wasmBindgen, [
    '--target',
    'web',
    // The module is always passed in; no fallback that would fetch a URL.
    '--omit-default-module-path',
    '--remove-producers-section',
    '--out-name',
    STEM,
    '--out-dir',
    workDir,
    wasmFile,
  ]);
  const declarations = readFileSync(path.join(workDir, `${STEM}.d.ts`), 'utf8');
  const exported = [...declarations.matchAll(/^export function (?!initSync\b)\w+\(.*\): .*;$/gm)]
    .map((match) => match[0])
    .sort();
  if (JSON.stringify(exported) !== JSON.stringify([...EXPECTED_EXPORTS].sort())) {
    throw new BuildError(
      `the exports of ${CRATE} changed:\n  ${exported.join('\n  ')}\n` +
        'Update src/glue.d.ts, src/core.ts and EXPECTED_EXPORTS in scripts/build.mjs together.',
    );
  }
  return {
    glue: readFileSync(path.join(workDir, `${STEM}.js`), 'utf8'),
    wasm: path.join(workDir, `${STEM}_bg.wasm`),
  };
}

/** Step 3: wasm-opt -Oz, or null when skipped. */
function optimise(wasmFile) {
  if (process.env.B2C_SKIP_WASM_OPT === '1') {
    return null;
  }
  const wasmOpt = tool('WASM_OPT', 'wasm-opt');
  let version;
  try {
    version = capture(wasmOpt, ['--version']).trim();
  } catch (error) {
    throw new BuildError(
      `${error.message}; install binaryen, set WASM_OPT, or set B2C_SKIP_WASM_OPT=1 for a local build`,
    );
  }
  // The options this wasm-opt knows, as whole words of its help text.
  const known = new Set(capture(wasmOpt, ['--help']).split(/\s+/));
  const missing = REQUIRED_FEATURES.filter((flag) => !known.has(flag));
  if (missing.length > 0) {
    throw new BuildError(
      `${wasmOpt} (${version}) does not know ${missing.join(', ')}; use a newer binaryen, ` +
        'or set B2C_SKIP_WASM_OPT=1 for a local build',
    );
  }
  const features = [...REQUIRED_FEATURES, ...OPTIONAL_FEATURES.filter((flag) => known.has(flag))];
  const optimised = path.join(workDir, `${STEM}_opt.wasm`);
  capture(wasmOpt, ['-Oz', ...features, '--output', optimised, wasmFile]);
  return { file: optimised, version };
}

/**
 * Step 4: wraps the generated glue module in `createGlue()`. The generated module keeps its
 * instance in module-level variables and refuses a second initialisation; inside a function,
 * every call gets fresh variables, so a trapped instance can be replaced. The shape of the
 * generated code is checked first, so a different wasm-bindgen output stops the build instead of
 * producing broken glue.
 */
function wrapGlue(source, bindgenVersion) {
  let body = source.replace(/^\/\* @ts-self-types="[^"]*" \*\/\n/, '');
  const functions = [...body.matchAll(/^export function (\w+)\s*\(/gm)].map((match) => match[1]);
  if (JSON.stringify(functions.sort()) !== JSON.stringify(EXPORTED_FUNCTIONS)) {
    throw new BuildError(`unexpected glue functions: ${functions.join(', ')}`);
  }
  body = body.replace(/^export function /gm, 'function ');
  const tail = 'export { initSync, __wbg_init as default };';
  if (body.split(tail).length !== 2) {
    throw new BuildError('the generated glue does not end with the expected export list');
  }
  body = body.replace(tail, '');
  const forbidden = [
    [/^\s*export\b/m, 'another export'],
    [/^\s*import\b/m, 'an import'],
    [/\bimport\s*\(/, 'a dynamic import'],
    [/\bimport\.meta\b/, 'import.meta'],
    [/\beval\s*\(/, 'eval'],
    [/\bnew\s+Function\b/, 'new Function'],
    [/^let wasm;$|^let wasmModule, wasmInstance, wasm;$/m, null],
  ];
  for (const [pattern, what] of forbidden) {
    const found = pattern.test(body);
    if (what === null ? !found : found) {
      throw new BuildError(
        what === null
          ? 'the generated glue does not keep its instance in a module-level variable'
          : `the generated glue contains ${what}`,
      );
    }
  }
  return [
    `// Generated by scripts/build.mjs from the wasm-bindgen ${bindgenVersion} --target web glue of`,
    `// crates/${CRATE}. Do not edit. The glue is wrapped in createGlue() so that every`,
    '// WebAssembly instance gets its own glue state (see src/loader.ts).',
    'export function createGlue() {',
    body.trim(),
    `  return { init: __wbg_init, ${EXPORTED_FUNCTIONS.join(', ')} };`,
    '}',
    '',
  ].join('\n');
}

function formatBytes(bytes) {
  return `${bytes.toLocaleString('en-US')} bytes`;
}

/** Steps 4–6 on the bindgen output; writes pkg/ only when the module is within the budget. */
function writePackage(glue, raw, finalBytes, optimised, bindgenVersion) {
  const base64 = finalBytes.toString('base64');
  const sizes = {
    raw: raw.length,
    optimised: optimised === null ? null : finalBytes.length,
    wasmOpt: optimised === null ? null : optimised.version,
    gzip: gzipSync(finalBytes, { level: 9 }).length,
    base64Chunk: base64.length,
    budget: SIZE_BUDGET_BYTES,
  };
  console.log(`wasm-bindgen output: ${formatBytes(sizes.raw)}`);
  console.log(
    optimised === null
      ? 'wasm-opt -Oz:         skipped (B2C_SKIP_WASM_OPT=1)'
      : `wasm-opt -Oz:         ${formatBytes(sizes.optimised)} (${optimised.version})`,
  );
  console.log(`gzip -9 (reported):   ${formatBytes(sizes.gzip)}`);
  console.log(`base64 chunk:         ${formatBytes(sizes.base64Chunk)}`);
  console.log(`budget:               ${formatBytes(SIZE_BUDGET_BYTES)} after wasm-opt`);
  if (finalBytes.length > SIZE_BUDGET_BYTES) {
    throw new BuildError(
      `the module is ${formatBytes(finalBytes.length)}, over the budget of ${formatBytes(SIZE_BUDGET_BYTES)}`,
    );
  }

  writeFileSync(path.join(pkgDir, 'glue.js'), wrapGlue(glue, bindgenVersion));
  writeFileSync(path.join(pkgDir, `${STEM}_bg.wasm`), finalBytes);
  writeFileSync(
    path.join(pkgDir, 'pkg-bytes.js'),
    `// Generated by scripts/build.mjs: the compiler core module (${finalBytes.length} bytes) as base64. Do not edit.\n` +
      `export const WASM_BASE64 = '${base64}';\n`,
  );
  writeFileSync(path.join(pkgDir, 'sizes.json'), `${JSON.stringify(sizes, null, 2)}\n`);
}

function main() {
  // A failed build leaves no pkg/ behind, so nothing stale is ever tested or bundled.
  rmSync(pkgDir, { recursive: true, force: true });
  try {
    const bindgenVersion = lockedWasmBindgenVersion();
    const built = cargoBuild();
    mkdirSync(workDir, { recursive: true });
    const { glue, wasm } = bindgen(built, bindgenVersion);
    const raw = readFileSync(wasm);
    const optimised = optimise(wasm);
    const finalBytes = optimised === null ? raw : readFileSync(optimised.file);
    writePackage(glue, raw, finalBytes, optimised, bindgenVersion);
    rmSync(workDir, { recursive: true, force: true });
  } catch (error) {
    rmSync(pkgDir, { recursive: true, force: true });
    throw error;
  }
}

try {
  main();
} catch (error) {
  if (error instanceof BuildError) {
    console.error(`b2c-core-wasm build failed: ${error.message}`);
    process.exit(1);
  }
  throw error;
}
