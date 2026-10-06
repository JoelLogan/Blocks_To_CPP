/** What a flow test may change about the app's launch. */
import { describe, expect, it } from 'vitest';

import { checkedOverrides, portOf } from './launch';

describe('checkedOverrides', () => {
  it('lets a test set the toolchain folders and the log level', () => {
    expect(checkedOverrides({ B2C_E2E_TOOLCHAIN_DIRS: '/tmp/x', B2C_LOG: 'debug' })).toEqual({
      B2C_E2E_TOOLCHAIN_DIRS: '/tmp/x',
      B2C_LOG: 'debug',
    });
    expect(checkedOverrides({})).toEqual({});
  });

  it('refuses the profile, the dialog script and anything else', () => {
    for (const name of ['B2C_E2E_ROOT', 'B2C_E2E_DIALOGS', 'B2C_E2E_APP', 'PATH', '__proto__']) {
      expect(() => checkedOverrides({ [name]: 'x' }), name).toThrow(`may not set ${name}`);
    }
  });
});

describe('portOf', () => {
  it('reads the port of a tauri-driver URL', () => {
    expect(portOf('http://127.0.0.1:43375/')).toBe(43375);
  });

  it('refuses a URL without a port', () => {
    expect(() => portOf('http://127.0.0.1/')).toThrow('no port');
  });
});
