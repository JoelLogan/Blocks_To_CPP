// The isolation hook (docs/spec/08-security.md §8.8). It runs in a sandboxed iframe on its own
// origin. Tauri calls it with every IPC message from the editor before encrypting the message for
// the backend. A message that is not on the allowlist is dropped here: the hook throws, so the
// message is never sent, and a compromised editor cannot reach any other command.
//
// index.html loads allowlist.generated.js (B2C_IPC_ALLOWLIST, generated from the Rust command
// table) and validate.js (b2cValidateIpcMessage) before this script. The backend checks every
// request again; this hook keeps everything the allowlist does not name away from it.
'use strict';
/* global B2C_IPC_ALLOWLIST, b2cValidateIpcMessage */

/**
 * The command name of a refused message, for the error text: only a short name made of the
 * characters command names use, read without running any getter. Anything else is not shown.
 * @param {unknown} message
 * @returns {string}
 */
function b2cRefusedCommand(message) {
  try {
    if (typeof message !== 'object' || message === null) {
      return '(not a message)';
    }
    const descriptor = Object.getOwnPropertyDescriptor(message, 'cmd');
    const command = descriptor && 'value' in descriptor ? descriptor.value : undefined;
    return typeof command === 'string' && /^[A-Za-z0-9_:|.-]{1,64}$/.test(command)
      ? command
      : '(not shown)';
  } catch {
    return '(not shown)';
  }
}

/**
 * @param {unknown} message - `{ cmd, callback, error, payload, options }` from the editor
 * @returns {unknown} the unchanged message, when it is allowed
 */
window.__TAURI_ISOLATION_HOOK__ = (message) => {
  if (!b2cValidateIpcMessage(message, B2C_IPC_ALLOWLIST)) {
    throw new Error(
      `Blocked an IPC message that is not on the allowlist (command: ${b2cRefusedCommand(message)})`,
    );
  }
  return message;
};
