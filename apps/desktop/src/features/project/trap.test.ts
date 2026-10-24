/**
 * Saving when the compiler core traps: the save starts a fresh core and carries on, so a
 * WebAssembly trap never costs the user their changes. (Its own file, because the restart replaces
 * the package's shared core.)
 *
 * Without a build of the compiler core this test is skipped, unless B2C_REQUIRE_WASM is set.
 */
import { CoreTrap, type CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { setCore } from '../../app/core';
import {
  CORE_BUILT,
  HANDLE_A,
  HELLO_TEXT,
  type Harness,
  installHarness,
  opened,
  realCore,
} from './testing';

let core: CoreWasm;
let harness: Harness;

beforeAll(async () => {
  if (CORE_BUILT) {
    core = await realCore();
  }
});

beforeEach(() => {
  harness = installHarness(CORE_BUILT ? core : null);
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

afterEach(() => {
  harness.dispose();
});

describe.skipIf(!CORE_BUILT)('saving when the compiler core traps', () => {
  it('starts a new core and saves', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    harness.ipc.projectOpenDialog.mockResolvedValueOnce({
      status: 'ok',
      ...opened(HELLO_TEXT, { handle: HANDLE_A }),
    });
    await harness.feature.lifecycle.open();
    const trapping: CoreWasm = {
      ...core,
      version: () => core.version(),
      canonical: () => {
        throw new CoreTrap('unreachable');
      },
    };
    setCore(trapping);
    harness.ipc.projectSave.mockResolvedValueOnce({
      savedAt: '2026-10-05T10:42:00Z',
      hash: 'f'.repeat(64),
    });

    expect(await harness.feature.lifecycle.save()).toBe(true);

    expect(harness.ipc.projectSave).toHaveBeenCalledWith({
      handle: HANDLE_A,
      document: HELLO_TEXT,
    });
    expect(error).toHaveBeenCalledWith(
      'The compiler core stopped while saving; starting a new one',
      expect.any(CoreTrap),
    );
  });
});
