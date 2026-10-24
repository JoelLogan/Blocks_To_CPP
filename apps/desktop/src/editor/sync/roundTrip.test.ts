/**
 * The round-trip property (docs/spec/09-quality-and-delivery.md §9.2 (d)): for every example and
 * every security-suite project that loads, BDM → headless Blockly → BDM gives a document whose
 * canonical text is byte for byte the canonical text of the input. Also for a rendered workspace.
 */
import { afterEach, describe, expect, it } from 'vitest';

import { loadModule } from './bdmToWorkspace';
import {
  acceptedSecurityProjects,
  canonicalText,
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  headlessWorkspace,
  loadText,
  renderedWorkspace,
  testCore,
} from './testing';
import { readModule } from './workspaceToBdm';

const core = await testCore();

afterEach(() => {
  disposeWorkspaces();
});

/**
 * Every example, and every security project the loader accepts (tests/security/projects/README.md),
 * by a readable name.
 */
const PROJECTS: [string, string][] = [
  ...Object.entries(EXAMPLE_PROJECTS).map(([name, text]): [string, string] => [
    `examples/${name}`,
    text,
  ]),
  ...acceptedSecurityProjects().map(([name, text]): [string, string] => [
    `tests/security/projects/${name}`,
    text,
  ]),
];

describe.skipIf(core === null)('BDM → Blockly → BDM', () => {
  it('covers every example and the security projects the loader accepts', () => {
    expect(Object.keys(EXAMPLE_PROJECTS).length).toBeGreaterThanOrEqual(15);
    expect(PROJECTS.filter(([name]) => name.startsWith('tests/')).length).toBeGreaterThanOrEqual(
      25,
    );
    for (const [name, text] of PROJECTS) {
      expect(core?.load(new TextEncoder().encode(text)).ok, name).toBe(true);
    }
  });

  it.each(PROJECTS)('keeps %s byte for byte (headless)', (_name, text) => {
    if (core === null) {
      return;
    }
    const doc = loadText(core, text);
    const expected = canonicalText(core, doc);
    const workspace = headlessWorkspace();
    let result = doc;
    for (const module of doc.modules) {
      loadModule(workspace, result, module.id);
      result = readModule(workspace, result, module.id);
    }
    expect(canonicalText(core, result)).toBe(expected);
  });

  it.each(PROJECTS)('keeps %s byte for byte (rendered)', (_name, text) => {
    if (core === null) {
      return;
    }
    const doc = loadText(core, text);
    const workspace = renderedWorkspace();
    let result = doc;
    for (const module of doc.modules) {
      loadModule(workspace, result, module.id);
      result = readModule(workspace, result, module.id);
    }
    expect(canonicalText(core, result)).toBe(canonicalText(core, doc));
  });
});
