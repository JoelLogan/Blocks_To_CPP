// What a build of the WebAssembly core was made from, and whether the source still matches it
// (docs/spec/06-compiler-pipeline.md §6.13). scripts/build.mjs records a SHA-256 fingerprint of
// the core's inputs in pkg/build-info.json; the desktop app's Vite plugin
// (apps/desktop/vite/wasmCore.ts) computes it again and refuses a production build of the editor
// when the two differ, so an app never embeds a core older than its source.
//
// The inputs, listed explicitly in INPUT_GROUPS (everything that can change the module):
//   - the root Cargo.toml (the wasm-release profile, workspace dependency versions and features);
//   - the Cargo.lock entries of b2c-core-wasm and everything it depends on, transitively (the
//     lockfile does not say which dependencies are for tests, so a test-only update counts too;
//     the desktop app's own dependencies do not);
//   - rust-toolchain.toml (the compiler);
//   - catalog/ (compiled into b2c-catalog with include_str!);
//   - for each crate in CORE_CRATES, its Cargo.toml and every file under its src/ (tests,
//     benches and examples outside src/ are not compiled into the module);
//   - this script and scripts/build.mjs.
// Not covered: the wasm-bindgen CLI (build.mjs checks its version against Cargo.lock), wasm-opt
// and B2C_SKIP_WASM_OPT (they change the module's size, not what it does), and RUSTFLAGS or a
// .cargo/config.toml, which the repository does not use.
//
// Only Node built-ins. Types: build-info.d.mts.

import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

/** The core package's folder, relative to the repository root. */
export const CORE_PACKAGE_DIR = 'packages/b2c-core-wasm';

/** The build's output folder, relative to the repository root (not committed). */
export const PKG_DIR = `${CORE_PACKAGE_DIR}/pkg`;

/** The file in pkg/ that records what the build was made from. */
export const BUILD_INFO_FILE = 'build-info.json';

/** The layout of build-info.json; a file with another one counts as unreadable. */
export const BUILD_INFO_VERSION = 1;

/**
 * The crates compiled into the module: b2c-core-wasm and the workspace crates it depends on
 * (test/build-info.test.ts checks this list against their Cargo.toml files).
 */
export const CORE_CRATES = Object.freeze([
  'b2c-core-wasm',
  'b2c-ir',
  'b2c-model',
  'b2c-catalog',
  'b2c-lang',
  'b2c-codegen',
]);

/** The crate whose Cargo.lock entries, with their dependencies, are an input. */
const ROOT_CRATE = 'b2c-core-wasm';

/** The name of the input group made of Cargo.lock entries. */
const LOCK_GROUP = 'Cargo.lock';

/**
 * The inputs in named groups, in a fixed order. Each path is a file or a folder (every file in
 * it, recursively), relative to the repository root; a missing path is an error, because the
 * list would then be out of date. The Cargo.lock group is the dependency closure of ROOT_CRATE.
 */
export const INPUT_GROUPS = Object.freeze(
  [
    { name: 'Cargo.toml', paths: ['Cargo.toml'] },
    { name: LOCK_GROUP, paths: [] },
    { name: 'rust-toolchain.toml', paths: ['rust-toolchain.toml'] },
    { name: 'catalog', paths: ['catalog'] },
    ...CORE_CRATES.map((crate) => ({
      name: `crates/${crate}`,
      paths: [`crates/${crate}/Cargo.toml`, `crates/${crate}/src`],
    })),
    {
      name: `${CORE_PACKAGE_DIR}/scripts`,
      paths: [
        `${CORE_PACKAGE_DIR}/scripts/build.mjs`,
        `${CORE_PACKAGE_DIR}/scripts/build-info.mjs`,
      ],
    },
  ].map((group) => Object.freeze({ ...group, paths: Object.freeze(group.paths) })),
);

/** An input that cannot be read, or a Cargo.lock this script does not understand. */
export class BuildInfoError extends Error {}

function sha256(data) {
  return createHash('sha256').update(data).digest('hex');
}

/** `relative` with forward slashes, so the fingerprint is the same on every system. */
function portable(relative) {
  return relative.split(path.sep).join('/');
}

/** Every file at `absolute` (the file itself, or the files in the folder), as absolute paths. */
function filesAt(absolute, relative) {
  let stats;
  try {
    stats = statSync(absolute);
  } catch (error) {
    throw new BuildInfoError(
      `the input ${relative} of the WebAssembly core is missing (${error.code ?? error.message}); ` +
        `update INPUT_GROUPS in ${CORE_PACKAGE_DIR}/scripts/build-info.mjs`,
      { cause: error },
    );
  }
  if (!stats.isDirectory()) {
    return [absolute];
  }
  return readdirSync(absolute).flatMap((name) =>
    filesAt(path.join(absolute, name), `${relative}/${name}`),
  );
}

/**
 * The packages of a Cargo.lock, each with the text of its `[[package]]` entry.
 *
 * @throws BuildInfoError when an entry has no name or version.
 */
export function parseLock(text) {
  const entries = [];
  let current = null;
  for (const line of text.replace(/\r\n/g, '\n').split('\n')) {
    if (line.startsWith('[')) {
      current = line === '[[package]]' ? [] : null;
      if (current !== null) {
        entries.push(current);
      }
    }
    current?.push(line);
  }
  return entries.map((lines) => {
    const entry = lines.join('\n').trimEnd();
    const name = /^name = "([^"]+)"$/m.exec(entry)?.[1];
    const version = /^version = "([^"]+)"$/m.exec(entry)?.[1];
    if (name === undefined || version === undefined) {
      throw new BuildInfoError(`Cargo.lock has a [[package]] entry without a name or version`);
    }
    const list = /^dependencies = \[\n([\s\S]*?)^\]$/m.exec(entry)?.[1] ?? '';
    const dependencies = [...list.matchAll(/^ "([^"]+)",$/gm)].map((match) => match[1]);
    return { name, version, dependencies, entry };
  });
}

/**
 * The Cargo.lock entries of `root` and of every package it depends on, transitively, sorted by
 * name and version.
 *
 * @throws BuildInfoError when `root` is not in the lockfile or a dependency cannot be resolved.
 */
export function lockClosure(text, root) {
  const packages = parseLock(text);
  const byName = new Map();
  for (const entry of packages) {
    byName.set(entry.name, [...(byName.get(entry.name) ?? []), entry]);
  }
  // A dependency is "name", or "name version" when several versions are locked, followed by
  // " (source)" when several sources are.
  const resolve = (reference) => {
    const [name, version] = reference.split(' ');
    const candidates = (byName.get(name) ?? []).filter(
      (entry) => version === undefined || entry.version === version,
    );
    if (candidates.length !== 1) {
      throw new BuildInfoError(
        `Cargo.lock: "${reference}" matches ${String(candidates.length)} packages, expected 1`,
      );
    }
    return candidates[0];
  };

  const seen = new Set();
  const pending = [resolve(root)];
  while (pending.length > 0) {
    const entry = pending.pop();
    if (!seen.has(entry)) {
      seen.add(entry);
      pending.push(...entry.dependencies.map(resolve));
    }
  }
  return [...seen].sort((a, b) =>
    a.name === b.name ? compare(a.version, b.version) : compare(a.name, b.name),
  );
}

/** A comparison of strings by code unit, the same everywhere (no locale). */
function compare(a, b) {
  if (a === b) {
    return 0;
  }
  return a < b ? -1 : 1;
}

/** The digest of one group of files: each file's portable path and the SHA-256 of its bytes. */
function filesDigest(repoRoot, paths) {
  const lines = paths
    .flatMap((relative) => filesAt(path.join(repoRoot, relative), relative))
    .map((absolute) => ({
      name: portable(path.relative(repoRoot, absolute)),
      digest: sha256(readFileSync(absolute)),
    }))
    .sort((a, b) => compare(a.name, b.name))
    .map(({ name, digest }) => `${name}\0${digest}\n`);
  return { digest: sha256(lines.join('')), files: lines.length };
}

/**
 * The fingerprint of the core's inputs in the checkout at `repoRoot`: one SHA-256 per input
 * group and one over all of them. It depends only on the inputs' contents and paths, not on
 * time stamps or the order a folder lists its files in.
 *
 * @throws BuildInfoError when an input is missing or Cargo.lock cannot be read.
 */
export function computeFingerprint(repoRoot) {
  const inputs = {};
  let files = 0;
  for (const group of INPUT_GROUPS) {
    if (group.name === LOCK_GROUP) {
      const lockPath = path.join(repoRoot, 'Cargo.lock');
      let text;
      try {
        text = readFileSync(lockPath, 'utf8');
      } catch (error) {
        throw new BuildInfoError(`Cargo.lock cannot be read (${error.code ?? error.message})`, {
          cause: error,
        });
      }
      const closure = lockClosure(text, ROOT_CRATE);
      inputs[group.name] = sha256(closure.map((entry) => `${entry.entry}\n`).join('\n'));
      files += 1;
    } else {
      const { digest, files: count } = filesDigest(repoRoot, group.paths);
      inputs[group.name] = digest;
      files += count;
    }
  }
  const fingerprint = sha256(
    INPUT_GROUPS.map((group) => `${group.name}\0${inputs[group.name]}\n`).join(''),
  );
  return { fingerprint, inputs, files };
}

/** The text of pkg/build-info.json for a build of `fingerprint` with these tools. */
export function buildInfoText(fingerprint, tools) {
  const info = {
    buildInfoVersion: BUILD_INFO_VERSION,
    fingerprint: fingerprint.fingerprint,
    inputs: fingerprint.inputs,
    wasmBindgen: tools.wasmBindgen,
    wasmOpt: tools.wasmOpt,
  };
  return `${JSON.stringify(info, null, 2)}\n`;
}

/** Whether `value` is a build-info record this script can compare. */
function isBuildInfo(value) {
  return (
    typeof value === 'object' &&
    value !== null &&
    value.buildInfoVersion === BUILD_INFO_VERSION &&
    typeof value.fingerprint === 'string' &&
    typeof value.inputs === 'object' &&
    value.inputs !== null
  );
}

/**
 * Whether the core built in `<repoRoot>/packages/b2c-core-wasm/pkg` matches the source:
 *
 * - `fresh`: its build-info.json records the inputs as they are now;
 * - `stale`: some inputs changed since (`changed` names their groups);
 * - `notBuilt`: there is no build (pkg/glue.js is missing);
 * - `unknown`: there is a build but no readable build-info.json of this layout (`reason` is
 *   `missing` or `unreadable`), as after a build by an older build script.
 *
 * @throws BuildInfoError when the current inputs cannot be read.
 */
export function checkBuild(repoRoot) {
  const pkgDir = path.join(repoRoot, PKG_DIR);
  if (!existsSync(path.join(pkgDir, 'glue.js'))) {
    return { state: 'notBuilt' };
  }
  let recorded;
  try {
    recorded = JSON.parse(readFileSync(path.join(pkgDir, BUILD_INFO_FILE), 'utf8'));
  } catch (error) {
    return { state: 'unknown', reason: error.code === 'ENOENT' ? 'missing' : 'unreadable' };
  }
  if (!isBuildInfo(recorded)) {
    return { state: 'unknown', reason: 'unreadable' };
  }
  const current = computeFingerprint(repoRoot);
  if (recorded.fingerprint === current.fingerprint) {
    return { state: 'fresh' };
  }
  const changed = INPUT_GROUPS.map((group) => group.name).filter(
    (name) => recorded.inputs[name] !== current.inputs[name],
  );
  return { state: 'stale', changed };
}

/** The command that rebuilds the core, from the repository root. */
export const REBUILD_COMMAND = 'pnpm wasm:build';

/**
 * What to tell a person about a build that is not `fresh` (`null` for a fresh one): what is
 * wrong and the command that fixes it.
 */
export function describeBuild(result) {
  const build = (again) =>
    `Build it${again} from the repository root with \`${REBUILD_COMMAND}\` (set ` +
    "B2C_SKIP_WASM_OPT=1 without binaryen's wasm-opt), then run this again. It needs the " +
    'wasm-bindgen CLI of the version in Cargo.lock: see ' +
    'BUILDING.md#the-webassembly-core-does-not-build.';
  switch (result.state) {
    case 'fresh':
      return null;
    case 'notBuilt':
      return (
        `The WebAssembly core of the editor has not been built (${PKG_DIR}/ is missing; it is ` +
        `not committed). ${build('')}`
      );
    case 'unknown':
      return (
        `The WebAssembly core in ${PKG_DIR}/ has ${
          result.reason === 'missing' ? 'no' : 'an unreadable'
        } ${BUILD_INFO_FILE}, so whether it matches the source is unknown (an older build ` +
        `script made it). ${build(' again')}`
      );
    case 'stale':
      return (
        `The WebAssembly core in ${PKG_DIR}/ is out of date: ${
          result.changed.length > 0 ? result.changed.join(', ') : 'the list of its inputs'
        } changed since it was built, so the editor would show C++ from the old compiler. ` +
        build(' again')
      );
    default:
      throw new TypeError(`unknown build state ${String(result.state)}`);
  }
}
