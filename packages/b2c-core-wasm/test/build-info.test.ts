// The fingerprint of the core's inputs (scripts/build-info.mjs), which scripts/build.mjs writes
// to pkg/build-info.json and the desktop app's Vite plugin compares with the source.

import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, describe, expect, it } from 'vitest';

import {
  BUILD_INFO_FILE,
  BuildInfoError,
  buildInfoText,
  checkBuild,
  computeFingerprint,
  CORE_CRATES,
  describeBuild,
  INPUT_GROUPS,
  lockClosure,
  PKG_DIR,
} from '../scripts/build-info.mjs';

const repoRoot = fileURLToPath(new URL('../../../', import.meta.url));

const scratch: string[] = [];

afterEach(() => {
  for (const dir of scratch.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** A copy of the core's inputs (and nothing else) in a fresh folder. */
function copyInputs(): string {
  const root = mkdtempSync(path.join(tmpdir(), 'b2c-build-info-'));
  scratch.push(root);
  const paths = ['Cargo.lock', ...INPUT_GROUPS.flatMap((group) => group.paths)];
  for (const relative of paths) {
    cpSync(path.join(repoRoot, relative), path.join(root, relative), { recursive: true });
  }
  return root;
}

/** Writes a pkg/ with glue.js and, unless `info` is null, build-info.json. */
function writePkg(root: string, info: string | null): void {
  const pkg = path.join(root, PKG_DIR);
  mkdirSync(pkg, { recursive: true });
  writeFileSync(path.join(pkg, 'glue.js'), 'export function createGlue() {}\n');
  if (info !== null) {
    writeFileSync(path.join(pkg, BUILD_INFO_FILE), info);
  }
}

const TOOLS = { wasmBindgen: '0.2.129', wasmOpt: null };

/** Appends a line to a file of the copy. */
function append(root: string, relative: string, text: string): void {
  const file = path.join(root, relative);
  writeFileSync(file, `${readFileSync(file, 'utf8')}${text}`);
}

/** Every file below `dir` (relative to it, with forward slashes). */
function filesBelow(dir: string): string[] {
  return readdirSync(dir, { recursive: true, encoding: 'utf8' })
    .filter((name) => statSync(path.join(dir, name)).isFile())
    .map((name) => name.split(path.sep).join('/'));
}

describe('the fingerprint', () => {
  it('is stable: the same for the same contents, wherever they are and whatever their times', () => {
    const first = computeFingerprint(repoRoot);
    expect(computeFingerprint(repoRoot)).toEqual(first);
    expect(first.fingerprint).toMatch(/^[0-9a-f]{64}$/);
    expect(Object.keys(first.inputs)).toEqual(INPUT_GROUPS.map((group) => group.name));

    const copy = copyInputs();
    const lib = path.join(copy, 'crates/b2c-lang/src/lib.rs');
    utimesSync(lib, new Date(2001, 0, 1), new Date(2001, 0, 1));
    expect(computeFingerprint(copy)).toEqual(first);
  });

  it.each(INPUT_GROUPS.filter((group) => group.name !== 'Cargo.lock').map((group) => [group.name]))(
    'changes when a file of %s changes, and only that group',
    (name) => {
      const copy = copyInputs();
      const before = computeFingerprint(copy);
      const group = INPUT_GROUPS.find((candidate) => candidate.name === name);
      const target = group?.paths.at(-1) ?? '';
      const file = statSync(path.join(copy, target)).isDirectory()
        ? `${target}/${filesBelow(path.join(copy, target)).sort()[0] ?? ''}`
        : target;
      append(copy, file, '\n');

      const after = computeFingerprint(copy);
      expect(after.fingerprint).not.toBe(before.fingerprint);
      const changed = Object.keys(after.inputs).filter(
        (key) => after.inputs[key] !== before.inputs[key],
      );
      expect(changed).toEqual([name]);
    },
  );

  it('changes when a file is added to a covered folder or renamed in it', () => {
    const copy = copyInputs();
    const before = computeFingerprint(copy).inputs['crates/b2c-codegen'];
    writeFileSync(path.join(copy, 'crates/b2c-codegen/src/new_module.rs'), '');
    const added = computeFingerprint(copy).inputs['crates/b2c-codegen'];
    expect(added).not.toBe(before);

    const catalog = computeFingerprint(copy).inputs['catalog'];
    cpSync(path.join(copy, 'catalog/core/io.toml'), path.join(copy, 'catalog/core/io2.toml'));
    rmSync(path.join(copy, 'catalog/core/io.toml'));
    expect(computeFingerprint(copy).inputs['catalog']).not.toBe(catalog);
  });

  it("changes with a Cargo.lock entry the core uses, not with the desktop app's", () => {
    const copy = copyInputs();
    const lockFile = path.join(copy, 'Cargo.lock');
    const lock = readFileSync(lockFile, 'utf8');
    const closure = lockClosure(lock, 'b2c-core-wasm');
    const before = computeFingerprint(copy);
    /** The lockfile with the checksum of `name`'s entry (one of `entries`) changed. */
    const withChecksumChanged = (name: string, entries: { name: string; entry: string }[]) => {
      const { entry } = entries.find((candidate) => candidate.name === name) ?? { entry: '' };
      expect(entry).toMatch(/^checksum = "/m);
      return lock.replace(entry, entry.replace(/^checksum = "/m, 'checksum = "0'));
    };

    // A crate only the desktop app uses.
    expect(closure.map((entry) => entry.name)).not.toContain('tauri');
    writeFileSync(lockFile, withChecksumChanged('tauri', lockClosure(lock, 'blocks2cpp-desktop')));
    expect(computeFingerprint(copy)).toEqual(before);

    // A crate compiled into the core (through b2c-model and b2c-core-wasm).
    writeFileSync(lockFile, withChecksumChanged('serde_json', closure));
    const after = computeFingerprint(copy);
    expect(after.inputs['Cargo.lock']).not.toBe(before.inputs['Cargo.lock']);
    expect(after.fingerprint).not.toBe(before.fingerprint);
  });

  it('does not change with files outside the inputs', () => {
    const copy = copyInputs();
    const before = computeFingerprint(copy);
    for (const relative of [
      'crates/b2c-model/tests/extra.rs',
      'crates/b2c-core-wasm/benches/extra.rs',
      'packages/b2c-core-wasm/README.md',
      'packages/b2c-core-wasm/src/extra.ts',
      'crates/b2c-app/src/lib.rs',
    ]) {
      mkdirSync(path.dirname(path.join(copy, relative)), { recursive: true });
      writeFileSync(path.join(copy, relative), 'not an input\n');
    }
    expect(computeFingerprint(copy)).toEqual(before);
  });

  it('refuses a missing input instead of leaving it out', () => {
    const copy = copyInputs();
    rmSync(path.join(copy, 'crates/b2c-ir/src'), { recursive: true });
    expect(() => computeFingerprint(copy)).toThrow(BuildInfoError);
    expect(() => computeFingerprint(copy)).toThrow(/crates\/b2c-ir\/src/);
  });
});

describe('the input list', () => {
  /** The workspace crates a crate's [dependencies] (not dev or build) name. */
  function workspaceDependencies(crate: string): string[] {
    const manifest = readFileSync(path.join(repoRoot, 'crates', crate, 'Cargo.toml'), 'utf8');
    const section = /^\[dependencies\]\n([\s\S]*?)(?=^\[)/m.exec(manifest)?.[1] ?? '';
    return [...section.matchAll(/^(b2c-[a-z-]+)[.\s=]/gm)].map((match) => match[1] ?? '');
  }

  it('names every crate compiled into the core: b2c-core-wasm and its workspace dependencies', () => {
    const closure = new Set<string>();
    const pending = ['b2c-core-wasm'];
    for (let crate = pending.pop(); crate !== undefined; crate = pending.pop()) {
      if (!closure.has(crate)) {
        closure.add(crate);
        pending.push(...workspaceDependencies(crate));
      }
    }
    expect([...CORE_CRATES].sort()).toEqual([...closure].sort());
  });

  /**
   * Files outside the crates' src/ that only their unit tests read (`#[cfg(test)]`), so they are
   * not compiled into the module. Any other include_str!/include_bytes! must be an input.
   */
  const TEST_ONLY_INCLUDES = new Set([
    'crates/b2c-model/tests/fixtures/semver-precedence.json',
    'docs/reference/diagnostics/analyser.md',
    'examples/hello_world.b2c',
  ]);

  it('covers every file the crates compile in with include_str! or include_bytes!', () => {
    const inputPaths = INPUT_GROUPS.flatMap((group) => group.paths);
    const covered = (file: string) =>
      inputPaths.some((input) => file === input || file.startsWith(`${input}/`));
    const uncovered: string[] = [];
    let includes = 0;
    for (const crate of CORE_CRATES) {
      const src = path.join(repoRoot, 'crates', crate, 'src');
      for (const name of filesBelow(src).filter((file) => file.endsWith('.rs'))) {
        const text = readFileSync(path.join(src, name), 'utf8');
        for (const match of text.matchAll(/include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)/g)) {
          const target = path.posix.normalize(
            path.posix.join('crates', crate, 'src', path.posix.dirname(name), match[1] ?? ''),
          );
          includes += 1;
          if (!covered(target) && !TEST_ONLY_INCLUDES.has(target)) {
            uncovered.push(`${target} (crates/${crate}/src/${name})`);
          }
        }
      }
    }
    expect(includes).toBeGreaterThan(TEST_ONLY_INCLUDES.size);
    expect(uncovered).toEqual([]);
  });
});

describe('Cargo.lock', () => {
  const LOCK = [
    '# This file is automatically @generated by Cargo.',
    'version = 4',
    '',
    '[[package]]',
    'name = "app"',
    'version = "0.1.0"',
    'dependencies = [',
    ' "core",',
    ' "dup 2.0.0",',
    ']',
    '',
    '[[package]]',
    'name = "core"',
    'version = "0.1.0"',
    'dependencies = [',
    ' "dup 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)",',
    ' "leaf",',
    ']',
    '',
    '[[package]]',
    'name = "dup"',
    'version = "1.0.0"',
    'source = "registry+https://github.com/rust-lang/crates.io-index"',
    '',
    '[[package]]',
    'name = "dup"',
    'version = "2.0.0"',
    'source = "registry+https://github.com/rust-lang/crates.io-index"',
    '',
    '[[package]]',
    'name = "leaf"',
    'version = "0.3.0"',
    '',
  ].join('\n');

  it('gives the closure of a package, resolving references with versions and sources', () => {
    expect(lockClosure(LOCK, 'core').map((entry) => `${entry.name} ${entry.version}`)).toEqual([
      'core 0.1.0',
      'dup 1.0.0',
      'leaf 0.3.0',
    ]);
    expect(lockClosure(LOCK, 'app')).toHaveLength(5);
  });

  it('refuses a reference it cannot resolve to exactly one package', () => {
    expect(() => lockClosure(LOCK, 'missing')).toThrow(BuildInfoError);
    expect(() => lockClosure(LOCK, 'dup')).toThrow(/matches 2 packages/);
    expect(() => lockClosure(LOCK.replace(' "leaf",', ' "gone",'), 'core')).toThrow(
      /"gone" matches 0 packages/,
    );
  });
});

describe('checking a build', () => {
  it('is fresh right after a build of the same inputs', () => {
    const copy = copyInputs();
    writePkg(copy, buildInfoText(computeFingerprint(copy), TOOLS));
    expect(checkBuild(copy)).toEqual({ state: 'fresh' });
    expect(describeBuild({ state: 'fresh' })).toBeNull();
  });

  it('is stale after a covered file changed, naming its group and the command', () => {
    const copy = copyInputs();
    writePkg(copy, buildInfoText(computeFingerprint(copy), TOOLS));
    append(copy, 'catalog/toolbox.toml', '# changed\n');
    append(copy, 'crates/b2c-lang/Cargo.toml', '# changed\n');

    const state = checkBuild(copy);
    expect(state).toEqual({ state: 'stale', changed: ['catalog', 'crates/b2c-lang'] });
    expect(describeBuild(state)).toContain('out of date: catalog, crates/b2c-lang changed');
    expect(describeBuild(state)).toContain('`pnpm wasm:build`');
  });

  it('is not built without pkg/, and unknown without a readable build-info.json', () => {
    const copy = copyInputs();
    expect(checkBuild(copy)).toEqual({ state: 'notBuilt' });
    expect(describeBuild({ state: 'notBuilt' })).toMatch(/has not been built .*Build it from/);

    writePkg(copy, null);
    expect(checkBuild(copy)).toEqual({ state: 'unknown', reason: 'missing' });
    expect(describeBuild(checkBuild(copy))).toContain('has no build-info.json');

    writePkg(copy, '{ not json');
    expect(checkBuild(copy)).toEqual({ state: 'unknown', reason: 'unreadable' });

    const fingerprint = computeFingerprint(copy);
    const otherLayout = JSON.parse(buildInfoText(fingerprint, TOOLS)) as Record<string, unknown>;
    otherLayout['buildInfoVersion'] = 2;
    writePkg(copy, JSON.stringify(otherLayout));
    expect(checkBuild(copy)).toEqual({ state: 'unknown', reason: 'unreadable' });
  });

  it('records the fingerprint and the tools in build-info.json', () => {
    const fingerprint = computeFingerprint(repoRoot);
    const info = JSON.parse(
      buildInfoText(fingerprint, { wasmBindgen: '0.2.129', wasmOpt: 'wasm-opt version 133' }),
    ) as unknown;
    expect(info).toEqual({
      buildInfoVersion: 1,
      fingerprint: fingerprint.fingerprint,
      inputs: fingerprint.inputs,
      wasmBindgen: '0.2.129',
      wasmOpt: 'wasm-opt version 133',
    });
  });
});
