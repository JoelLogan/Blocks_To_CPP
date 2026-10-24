// Tests of the all-Markdown check (`node build.mjs --check-repo-markdown`).
// Run with `pnpm --filter @blocks2cpp/site test` (node --test).
//
// Each fixture is a scratch git repository in the system's temporary folder, so
// the check runs exactly as in CI: on the files git lists.

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { checkRepositoryMarkdown } from './build.mjs';

const REPO_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** The source files whose messages the check reads; every fixture needs them. */
const SOURCE_FILES = {
  'apps/desktop/vite.config.ts': '// no references\n',
  'packages/b2c-core-wasm/scripts/build.mjs': '// no references\n',
  'packages/b2c-core-wasm/scripts/build-info.mjs': '// no references\n',
};

const scratchDirs = [];
after(() => {
  for (const dir of scratchDirs) rmSync(dir, { recursive: true, force: true });
});

/** Writes `files` (path → text) into a new git repository and returns its folder. */
function repository(files) {
  const dir = mkdtempSync(path.join(tmpdir(), 'b2c-anchor-check-'));
  scratchDirs.push(dir);
  execFileSync('git', ['init', '--quiet'], { cwd: dir });
  write(dir, { ...SOURCE_FILES, ...files });
  return dir;
}

/** Writes or replaces files in a fixture repository. */
function write(dir, files) {
  for (const [file, text] of Object.entries(files)) {
    const absolute = path.join(dir, ...file.split('/'));
    mkdirSync(path.dirname(absolute), { recursive: true });
    writeFileSync(absolute, text);
  }
}

/** Adds every file to git's index, as for a commit. */
function track(dir) {
  execFileSync('git', ['add', '--all'], { cwd: dir });
}

const DESKTOP_README = '# Desktop app\n\n## Requirements\n\nText.\n\n## Develop\n\nSee [requirements](#requirements).\n';

describe('the all-Markdown check', () => {
  test("passes on the repository's own Markdown", () => {
    const { files, problems } = checkRepositoryMarkdown(REPO_DIR);
    assert.deepEqual(problems, []);
    assert.ok(files > 50, `only ${files} Markdown files were found`);
  });

  test('fails when a heading another file links to is renamed', () => {
    const dir = repository({
      'README.md': '# Project\n\nSet up as [described](apps/desktop/README.md#develop).\n',
      'apps/desktop/README.md': DESKTOP_README,
    });
    track(dir);
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, []);

    write(dir, { 'apps/desktop/README.md': DESKTOP_README.replace('## Develop', '## Developing') });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, [
      'README.md:3: link "apps/desktop/README.md#develop" points to a heading that does not exist in ' +
        '"apps/desktop/README.md"',
    ]);
  });

  test("checks anchors into the file's own headings", () => {
    const dir = repository({
      'apps/desktop/README.md': DESKTOP_README.replace('## Requirements', '## Prerequisites'),
    });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, [
      'apps/desktop/README.md:9: link "#requirements" points to a heading that does not exist in ' +
        '"apps/desktop/README.md"',
    ]);
  });

  test("follows GitHub's anchors for repeated headings, punctuation and code", () => {
    const dir = repository({
      'doc.md': [
        '# Doc',
        '## Tests',
        '## Tests',
        '## 6.2 Stage ① Load and validate',
        '## The `b2c` tool',
        '',
        '[a](#tests) [b](#tests-1) [c](#62-stage--load-and-validate) [d](#the-b2c-tool) [e](#tests-2)',
        '',
      ].join('\n'),
    });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, [
      'doc.md:7: link "#tests-2" points to a heading that does not exist in "doc.md"',
    ]);
  });

  test('reports links to missing files, outside the repository, and malformed ones', () => {
    const dir = repository({
      'docs/guide.md': [
        '# Guide',
        '',
        '- [gone](missing.md)',
        '- [up](../../outside.md)',
        '- [escape](%E0%A4%A.md)',
        '- [windows](..\\README.md)',
        '- ![picture](images/missing.png)',
        '- [source view](../README.md?plain=1#intro)',
        '- [reference][gone]',
        '',
        '[gone]: other/missing.md#part',
        '',
      ].join('\n'),
      'README.md': '# Readme\n\n## Intro\n',
    });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, [
      'docs/guide.md:3: link "missing.md" points to "docs/missing.md", which does not exist',
      'docs/guide.md:4: link "../../outside.md" points outside the repository',
      'docs/guide.md:5: link "%E0%A4%A.md" contains a malformed %-escape',
      'docs/guide.md:6: link "..\\README.md" uses a backslash; use "/" between path segments',
      'docs/guide.md:7: link "images/missing.png" points to "docs/images/missing.png", which does not exist',
      'docs/guide.md:8: link "../README.md?plain=1#intro" uses ?plain=1, which only supports line anchors such as #L10',
      'docs/guide.md:9: link "other/missing.md#part" points to "docs/other/missing.md", which does not exist',
    ]);
  });

  test('accepts root-relative links, folders, line anchors and fragments on other files', () => {
    const dir = repository({
      'docs/guide.md': [
        '# Guide',
        '',
        '[root](/README.md#intro) [folder](../examples/) [lines](../README.md?plain=1#L2-L3) [licence](../LICENSE#L5)',
        '',
      ].join('\n'),
      'README.md': '# Readme\n\n## Intro\n',
      'LICENSE': 'Licence text\n',
      'examples/hello.b2c': '{}\n',
    });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, []);
  });

  test('leaves external links to lychee', () => {
    const dir = repository({
      'README.md':
        '# Readme\n\n[web](https://example.com/page#nowhere) [mail](mailto:someone@example.com) ' +
        '[plain](http://localhost:1420/#x) <https://example.com/#y>\n',
    });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, []);
  });

  test('checks new files git does not ignore, and skips ignored and excluded paths', () => {
    const broken = '# Broken\n\n[x](#missing)\n';
    const dir = repository({
      '.gitignore': 'dist/\n',
      'new.md': broken,
      'dist/out.md': broken,
      'node_modules/pkg/README.md': broken,
      'site/node_modules/pkg/README.md': broken,
      '.claude/worktrees/w/README.md': broken,
      'fuzz/corpus/target/seed.md': broken,
      'fuzz/artifacts/target/crash.md': broken,
    });
    track(dir);
    const { files, problems } = checkRepositoryMarkdown(dir);
    assert.deepEqual(problems, ['new.md:3: link "#missing" points to a heading that does not exist in "new.md"']);
    assert.equal(files, 1);
  });

  test("checks CHANGELOG.md's unreleased changes but not its released history", () => {
    const dir = repository({
      'CHANGELOG.md': [
        '# Changelog',
        '',
        '## [Unreleased]',
        '',
        '- New: see [the guide](docs/guide.md#new-heading).',
        '',
        '## [0.1.0] - 2026-01-01',
        '',
        '- Old: see [the guide](docs/guide.md#old-heading).',
        '',
      ].join('\n'),
      'docs/guide.md': '# Guide\n',
    });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, [
      'CHANGELOG.md:5: link "docs/guide.md#new-heading" points to a heading that does not exist in "docs/guide.md"',
    ]);
  });

  test("checks the Markdown references in the source files' messages", () => {
    const dir = repository({
      'BUILDING.md': '# Building\n\n## Build the WebAssembly core\n',
      'apps/desktop/vite.config.ts':
        "this.error('Build it first: see BUILDING.md#build-the-webassembly-core.');\n" +
        "// Also docs/spec/missing.md and https://example.com/README.md#elsewhere\n",
      'packages/b2c-core-wasm/scripts/build.mjs': "const hint = 'see BUILDING.md#install-the-tools';\n",
    });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, [
      'apps/desktop/vite.config.ts:2: link "docs/spec/missing.md" points to "docs/spec/missing.md", which does not exist',
      'packages/b2c-core-wasm/scripts/build.mjs:1: link "BUILDING.md#install-the-tools" points to a heading that ' +
        'does not exist in "BUILDING.md"',
    ]);
  });

  test('reports a source file that is listed but missing', () => {
    const dir = repository({ 'README.md': '# Readme\n' });
    rmSync(path.join(dir, 'apps', 'desktop', 'vite.config.ts'));
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, [
      'apps/desktop/vite.config.ts: does not exist; update SOURCE_FILES_WITH_DOC_REFERENCES in site/build.mjs',
    ]);
  });

  test('reports each use of a broken link on its own line', () => {
    const dir = repository({
      'README.md': '# Readme\n\n[a](#gone)\n\nText.\n\n| Col |\n| --- |\n| [a](#gone) |\n',
    });
    assert.deepEqual(checkRepositoryMarkdown(dir).problems, [
      'README.md:3: link "#gone" points to a heading that does not exist in "README.md"',
      'README.md:9: link "#gone" points to a heading that does not exist in "README.md"',
    ]);
  });
});
