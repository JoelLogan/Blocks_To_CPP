// The isolation hook (docs/spec/08-security.md §8.8). It runs in a sandboxed iframe on its own
// origin. Tauri calls it with every IPC message from the editor before encrypting the message for
// the backend. A message that is not on the allowlist is dropped here: the hook throws, so the
// message is never sent, and a compromised editor cannot reach any other command.
'use strict';

/**
 * Whether `value` is an ordinary object (no class instance, array or exotic prototype).
 * @param {unknown} value
 * @returns {value is Record<string, unknown>}
 */
function isPlainObject(value) {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

/**
 * Whether `value` is an object without any keys: the arguments of a command that takes none.
 * @param {unknown} value
 */
function isNoArguments(value) {
  return isPlainObject(value) && Object.keys(value).length === 0;
}

/**
 * Every command the editor may call, with a check of its arguments. Keep this in step with
 * `generate_handler!` in ../src/main.rs and the permissions in ../capabilities/.
 * @type {ReadonlyMap<string, (payload: unknown) => boolean>}
 */
const ALLOWED_COMMANDS = new Map([['app_version', isNoArguments]]);

/**
 * @param {unknown} message - `{ cmd, callback, error, payload, options }` from the editor
 * @returns {unknown} the unchanged message, when it is allowed
 */
window.__TAURI_ISOLATION_HOOK__ = (message) => {
  const command = isPlainObject(message) ? message.cmd : undefined;
  const check = typeof command === 'string' ? ALLOWED_COMMANDS.get(command) : undefined;
  if (check === undefined || !isPlainObject(message) || !check(message.payload)) {
    throw new Error(
      `Blocked an IPC message that is not on the allowlist (command: ${String(command)})`,
    );
  }
  return message;
};
