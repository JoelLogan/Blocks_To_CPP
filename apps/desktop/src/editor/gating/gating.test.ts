import { BLOCK_DEFS, catalogBlock, TOOLBOX } from '@blocks2cpp/blockly-ext';
import type { ToolchainCapabilities } from '@blocks2cpp/ipc-types';
import { beforeEach, describe, expect, it } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import {
  documentFixture,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from '../../app/testing/fixtures';
import {
  AWAITED,
  blockGate,
  blockLabelParts,
  categoryGate,
  CPP_OPTION_LABELS,
  type EditorMode,
  hasFullCppLabel,
  editorMode,
  gate,
  gateText,
  labelParts,
  labelTemplate,
  modeKey,
  optionGate,
  optionLabel,
  partsMissingFromCppLabel,
  REQUIREMENTS,
  requirementKey,
  standardAtLeast,
  typeGate,
} from '.';

const CAPABLE: ToolchainCapabilities = {
  standards: ['c++17', 'c++20', 'c++23'],
  stdFormat: true,
  sanitizers: true,
  sarif: true,
};

function mode(overrides: Partial<EditorMode> = {}): EditorMode {
  return {
    labels: 'friendly',
    showAdvanced: false,
    standard: 'c++20',
    toolchain: CAPABLE,
    ...overrides,
  };
}

describe('gate', () => {
  it('allows what has no requirement', () => {
    expect(gate(undefined, mode())).toEqual({ allowed: true });
    expect(gate({}, mode())).toEqual({ allowed: true });
  });

  it('hides advanced things until Show advanced blocks is on', () => {
    expect(gate({ advanced: true }, mode())).toEqual({ allowed: false, reason: 'advanced' });
    expect(gate({ advanced: true }, mode({ showAdvanced: true }))).toEqual({ allowed: true });
    // Advanced comes first, even with a fallback.
    expect(gate({ advanced: true, fallback: true }, mode())).toMatchObject({ reason: 'advanced' });
  });

  it('needs the standard, or a fallback', () => {
    const needs23 = { standard: 'c++23' } as const;
    expect(gate(needs23, mode())).toEqual({ allowed: false, reason: 'standard', needs: 'c++23' });
    expect(gate(needs23, mode({ standard: 'c++23' }))).toEqual({ allowed: true });
    expect(gate(needs23, mode({ standard: 'c++26' }))).toEqual({ allowed: true });
    expect(gate({ ...needs23, fallback: true }, mode({ standard: 'c++17' }))).toEqual({
      allowed: true,
    });
  });

  it('needs the capability when the toolchain is known, or a fallback', () => {
    const format = { standard: 'c++20', capability: 'stdFormat' } as const;
    const old = mode({ toolchain: { ...CAPABLE, stdFormat: false } });
    expect(gate(format, old)).toEqual({ allowed: false, reason: 'toolchain', needs: 'stdFormat' });
    expect(gate(format, mode())).toEqual({ allowed: true });
    // Not known yet: the build reports it, the toolbox hides nothing.
    expect(gate(format, mode({ toolchain: null }))).toEqual({ allowed: true });
    expect(gate({ ...format, fallback: true }, old)).toEqual({ allowed: true });
    // The standard is checked first.
    expect(gate(format, { ...old, standard: 'c++17' })).toMatchObject({ reason: 'standard' });
  });

  it('orders the standards', () => {
    expect(standardAtLeast('c++20', 'c++17')).toBe(true);
    expect(standardAtLeast('c++20', 'c++20')).toBe(true);
    expect(standardAtLeast('c++17', 'c++20')).toBe(false);
    expect(standardAtLeast('c++26', 'c++23')).toBe(true);
  });

  it('says why something is not offered', () => {
    expect(gateText({ allowed: true })).toBeNull();
    expect(gateText({ allowed: false, reason: 'advanced' })).toBe(
      'Shown with Show advanced blocks (Settings).',
    );
    expect(gateText({ allowed: false, reason: 'standard', needs: 'c++23' })).toBe(
      "Needs C++23 or newer (the project's C++ standard).",
    );
    expect(gateText({ allowed: false, reason: 'toolchain', needs: 'stdFormat' })).toBe(
      'Needs a compiler with std::format (GCC 13 or newer).',
    );
  });
});

describe('the requirements table', () => {
  it('gates the rows the plans name', () => {
    expect(optionGate('var.change', 'OP', 'xor', mode())).toMatchObject({ reason: 'advanced' });
    expect(optionGate('var.change', 'OP', 'xor', mode({ showAdvanced: true })).allowed).toBe(true);
    expect(optionGate('var.change', 'OP', 'add', mode()).allowed).toBe(true);
    expect(blockGate('text.format', mode({ standard: 'c++17' }))).toMatchObject({
      reason: 'standard',
      needs: 'c++20',
    });
    expect(categoryGate('memory', mode())).toMatchObject({ reason: 'advanced' });
    expect(categoryGate('variables', mode()).allowed).toBe(true);
    expect(blockGate('var.declare', mode()).allowed).toBe(true);
    expect(typeGate('int', mode()).allowed).toBe(true);
  });

  it("refuses an option of a block that is itself refused, for the block's reason", () => {
    const old = mode({ standard: 'c++17' });
    expect(optionGate('text.format', 'ANY', 'x', old)).toEqual(blockGate('text.format', old));
  });

  it('names a block, option or category of the catalog in every row, or says what brings it', () => {
    const categories = new Set<string>(TOOLBOX.map((category) => category.id));
    for (const key of REQUIREMENTS.keys()) {
      if (AWAITED.has(key)) {
        continue;
      }
      const block = /^block:(.+)$/.exec(key);
      const option = /^option:([^.]+\.[^.]+)\.([A-Z][A-Z0-9_]*)=(.+)$/.exec(key);
      const category = /^category:(.+)$/.exec(key);
      if (block?.[1] !== undefined) {
        expect(catalogBlock(block[1]), key).toBeDefined();
      } else if (option?.[1] !== undefined) {
        const field = catalogBlock(option[1])?.fields.find((f) => f.name === option[2]);
        expect(
          field?.options.some(([, value]) => value === option[3]),
          key,
        ).toBe(true);
      } else if (category?.[1] !== undefined) {
        expect(categories.has(category[1]), key).toBe(true);
      } else {
        expect(key.startsWith('type:'), key).toBe(true);
      }
    }
    for (const key of AWAITED.keys()) {
      expect(REQUIREMENTS.has(key), key).toBe(true);
    }
  });

  it('builds its keys', () => {
    expect(requirementKey('block', 'text.format')).toBe('block:text.format');
    expect(requirementKey('option', 'var.change', 'OP', 'xor')).toBe('option:var.change.OP=xor');
    expect(requirementKey('category', 'memory')).toBe('category:memory');
    expect(requirementKey('type', '{"array":["int",3]}')).toBe('type:{"array":["int",3]}');
  });
});

describe('labels', () => {
  it('splits every Friendly label as the catalog generator does', () => {
    for (const def of BLOCK_DEFS) {
      expect(labelParts(def.label.friendly), def.id).toEqual(def.labelParts);
      expect(blockLabelParts(def, 'friendly'), def.id).toBe(def.labelParts);
    }
  });

  it('names only parts of the block in every C++ label', () => {
    for (const def of BLOCK_DEFS) {
      const names = new Set(
        [...def.fields, ...def.inputs, ...def.statements].map((part) => part.name),
      );
      for (const part of labelParts(def.label.cpp)) {
        if ('arg' in part) {
          expect(names.has(part.arg), `${def.id}: %${part.arg}`).toBe(true);
        }
      }
    }
  });

  it('keeps punctuation with its text and finds repeat markers only as words', () => {
    expect(labelParts('%LIST.push_back(%VALUE);')).toEqual([
      { arg: 'LIST' },
      { text: '.push_back(' },
      { arg: 'VALUE' },
      { text: ');' },
    ]);
    expect(labelParts('%ITEM && …')).toEqual([{ arg: 'ITEM' }, { text: '&&' }, { repeat: true }]);
    expect(labelParts('{%ITEM, …}')).toEqual([{ text: '{' }, { arg: 'ITEM' }, { text: ', …}' }]);
    expect(labelParts('100% %x  of\t%A')).toEqual([{ text: '100% %x of' }, { arg: 'A' }]);
  });

  it('chooses the template by mode', () => {
    const def = catalogBlock('coll.add');
    expect(def).toBeDefined();
    if (def === undefined) {
      return;
    }
    expect(labelTemplate(def, 'friendly')).toBe(def.label.friendly);
    expect(labelTemplate(def, 'cpp')).toBe(def.label.cpp);
    expect(hasFullCppLabel(def)).toBe(true);
    expect(blockLabelParts(def, 'cpp')).toEqual(labelParts(def.label.cpp));
  });

  it('keeps the Friendly layout for a C++ label that stands for one form only', () => {
    const def = catalogBlock('math.fn');
    expect(def).toBeDefined();
    if (def === undefined) {
      return;
    }
    expect(hasFullCppLabel(def)).toBe(false);
    expect(blockLabelParts(def, 'cpp')).toBe(def.labelParts);
    // Statement inputs need not be named: `{ … }` stands for them.
    const repeat = catalogBlock('control.repeat');
    expect(repeat !== undefined && hasFullCppLabel(repeat)).toBe(true);
  });

  it('lists the parts a C++ label leaves out', () => {
    expect(
      partsMissingFromCppLabel({
        label: { friendly: '%FN of %VALUE', cpp: 'std::round(%VALUE)' },
        fields: [{ name: 'FN' }],
        inputs: [{ name: 'VALUE' }],
        statements: [],
      } as unknown as Parameters<typeof partsMissingFromCppLabel>[0]),
    ).toEqual(['FN']);
  });

  it('shows each operator in the C++ mode and keeps the Friendly text otherwise', () => {
    expect(optionLabel('math.compare', 'OP', 'le', '≤', 'cpp')).toBe('<=');
    expect(optionLabel('math.compare', 'OP', 'le', '≤', 'friendly')).toBe('≤');
    expect(optionLabel('var.change', 'OP', 'add', 'by', 'cpp')).toBe('+=');
    expect(optionLabel('io.print', 'SEP', 'none', 'no separator', 'cpp')).toBe('no separator');
    expect(optionLabel('math.compare', 'OP', 'toString', 'x', 'cpp')).toBe('x');
  });

  it('has a C++ text for every option of the menus it covers', () => {
    for (const [key, table] of Object.entries(CPP_OPTION_LABELS)) {
      const dot = key.lastIndexOf('.');
      const def = catalogBlock(key.slice(0, dot));
      const field = def?.fields.find((candidate) => candidate.name === key.slice(dot + 1));
      expect(field, key).toBeDefined();
      for (const [, value] of field?.options ?? []) {
        expect(Object.hasOwn(table, value), `${key}=${value}`).toBe(true);
      }
    }
  });
});

describe('the editor mode', () => {
  beforeEach(() => {
    resetAppStore();
  });

  it('is Friendly, basic and C++20 before anything is read', () => {
    const current = editorMode(useAppStore.getState());
    expect(current).toEqual({
      labels: 'friendly',
      showAdvanced: false,
      standard: 'c++20',
      toolchain: null,
    });
    expect(modeKey(current)).toBe('friendly|basic|c++20|unknown');
  });

  it('follows the settings, the project and the toolchain a build would use', () => {
    const { actions } = useAppStore.getState();
    actions.setSettings({
      value: settingsFixture({
        editor: { blockLabels: 'cpp', showAdvanced: false },
        newProject: { standard: 'c++23' },
      }),
    });
    expect(editorMode(useAppStore.getState())).toMatchObject({ labels: 'cpp', standard: 'c++23' });

    const document = documentFixture();
    document.project.language.standard = 'c++17';
    document.project.options.showAdvanced = true;
    actions.setProject(projectFixture({ document }));
    actions.setToolchains({ list: [toolchainFixture()] });
    const current = editorMode(useAppStore.getState());
    expect(current).toMatchObject({ labels: 'cpp', showAdvanced: true, standard: 'c++17' });
    expect(current.toolchain).toEqual(toolchainFixture().capabilities);
    expect(modeKey(current)).toMatch(/^cpp\|advanced\|c\+\+17\|(fmt|nofmt)$/);
  });

  it('shows advanced blocks when the machine setting asks, whatever the project says', () => {
    const { actions } = useAppStore.getState();
    actions.setSettings({
      value: settingsFixture({ editor: { blockLabels: 'friendly', showAdvanced: true } }),
    });
    actions.setProject(projectFixture());
    expect(editorMode(useAppStore.getState()).showAdvanced).toBe(true);
  });

  it('changes its key with each part', () => {
    const base = mode();
    const keys = new Set([
      modeKey(base),
      modeKey({ ...base, labels: 'cpp' }),
      modeKey({ ...base, showAdvanced: true }),
      modeKey({ ...base, standard: 'c++23' }),
      modeKey({ ...base, toolchain: { ...CAPABLE, stdFormat: false } }),
      modeKey({ ...base, toolchain: null }),
    ]);
    expect(keys.size).toBe(6);
    // Capabilities the gate does not use do not change it.
    expect(modeKey({ ...base, toolchain: { ...CAPABLE, sarif: false } })).toBe(modeKey(base));
  });
});
