/** The ID generator (src/ids.ts). */
import * as Blockly from 'blockly/core';
import { describe, expect, it, vi } from 'vitest';

import {
  GENERATED_ID_PATTERN,
  IdGeneratorError,
  PROJECT_ID_PATTERN,
  installIdGenerator,
  isIdGeneratorInstalled,
  isProjectId,
  newId,
  type IdKind,
} from '../src';
import { headlessWorkspace, setUpBlocks } from './helpers';

describe('newId', () => {
  it('makes IDs of every kind with a prefix and 17 base62 characters', () => {
    for (const kind of ['blk', 'sym', 'mod', 'prj'] as const satisfies readonly IdKind[]) {
      const id = newId(kind);
      expect(id).toMatch(GENERATED_ID_PATTERN[kind]);
      expect(id).toMatch(PROJECT_ID_PATTERN);
      expect(id).toHaveLength(21);
    }
  });

  it('uses every base62 character and nothing else', () => {
    const seen = new Set<string>();
    for (let round = 0; round < 2_000; round++) {
      for (const character of newId('sym').slice(4)) {
        seen.add(character);
      }
    }
    expect(seen.size).toBe(62);
  });

  it('uses the CSPRNG and refuses to run without one', () => {
    const spy = vi.spyOn(globalThis.crypto, 'getRandomValues');
    newId('blk');
    expect(spy).toHaveBeenCalled();
    vi.stubGlobal('crypto', undefined);
    expect(() => newId('blk')).toThrow(IdGeneratorError);
  });
});

describe('isProjectId', () => {
  it('accepts what the project format accepts', () => {
    expect(isProjectId('b011')).toBe(true);
    expect(isProjectId('s_secret')).toBe(true);
    expect(isProjectId('a'.repeat(32))).toBe(true);
    for (const bad of ['', 'a-b', 'a b', '<x>', 'é', 'a'.repeat(33), 7, null]) {
      expect(isProjectId(bad)).toBe(false);
    }
  });
});

describe('installIdGenerator', () => {
  it("replaces Blockly's generator: 10,000 IDs match the pattern and are unique", () => {
    installIdGenerator();
    expect(isIdGeneratorInstalled()).toBe(true);
    const ids = new Set<string>();
    for (let round = 0; round < 10_000; round++) {
      const id = Blockly.utils.idGenerator.genUid();
      expect(id).toMatch(/^blk_[A-Za-z0-9]{17}$/);
      ids.add(id);
    }
    expect(ids.size).toBe(10_000);
  });

  it('is idempotent', () => {
    installIdGenerator();
    installIdGenerator();
    expect(isIdGeneratorInstalled()).toBe(true);
  });

  it('gives new, duplicated and pasted blocks blk_ IDs', () => {
    setUpBlocks();
    const workspace = headlessWorkspace();
    const block = workspace.newBlock('math.number');
    expect(block.id).toMatch(GENERATED_ID_PATTERN.blk);
    const state = Blockly.serialization.blocks.save(block, { addCoordinates: true });
    expect(state).not.toBeNull();
    if (state === null) {
      return;
    }
    const copy = Blockly.serialization.blocks.append({ ...state, id: block.id }, workspace);
    expect(copy.id).not.toBe(block.id);
    expect(copy.id).toMatch(GENERATED_ID_PATTERN.blk);
    const fresh = Blockly.serialization.blocks.append({ type: 'math.number' }, workspace);
    expect(fresh.id).toMatch(GENERATED_ID_PATTERN.blk);
  });

  it('fails loudly when Blockly ignores the replacement', () => {
    const spy = vi.spyOn(Blockly.utils.idGenerator, 'genUid').mockReturnValue('!#$%');
    expect(() => {
      installIdGenerator();
    }).toThrow(IdGeneratorError);
    spy.mockRestore();
  });
});
