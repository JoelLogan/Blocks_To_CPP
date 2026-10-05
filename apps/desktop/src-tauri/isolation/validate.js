// The IPC message validator of the isolation hook (docs/spec/08-security.md §8.8).
//
// A classic script for the isolation frame, loaded after allowlist.generated.js and before
// index.js. b2cValidateIpcMessage(message, allowlist) decides whether one IPC message from the
// editor may reach the backend. It accepts only:
//
// * a command on the allowlist whose payload has exactly the argument keys the allowlist names,
//   each of the right shape: JSON types, string lengths, ID formats, integer ranges and enum
//   values, with no unknown key and no `__proto__`, `constructor` or `prototype` key at any
//   depth, and only plain objects;
// * Tauri's channel-fetch command, which large channel messages use, with a null payload.
//
// Everything else returns false. The backend checks every request again; this check keeps a
// compromised editor away from everything the allowlist does not name.
'use strict';
/* exported b2cValidateIpcMessage */

/**
 * Whether one IPC message may be sent to the backend.
 * @param {unknown} message - `{ cmd, callback, error, payload, options }` from the editor
 * @param {unknown} allowlist - `B2C_IPC_ALLOWLIST`
 * @returns {boolean}
 */
function b2cValidateIpcMessage(message, allowlist) {
  const MESSAGE_KEYS = ['cmd', 'callback', 'error', 'payload', 'options'];
  const RESERVED_KEYS = ['__proto__', 'constructor', 'prototype'];
  // Tauri 2 fetches channel messages of 8 KiB or more (1 KiB for raw bytes) with this command
  // (tauri/src/ipc/channel.rs), passing the message's ID in a header.
  const FETCH_COMMAND = 'plugin:__TAURI_CHANNEL__|fetch';
  const FETCH_HEADER = 'Tauri-Channel-Id';
  const CHANNEL = /^__CHANNEL__:[0-9]{1,10}$/;
  const DIGITS = /^[0-9]{1,10}$/;
  const HEX = /^[0-9a-f]*$/;
  const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;
  const MAX_CALLBACK_ID = 0xffffffff;

  /** @type {(object: object, key: string) => boolean} */
  const hasOwn = (object, key) => Object.prototype.hasOwnProperty.call(object, key);

  /**
   * An ordinary object of this realm: no array, class instance or exotic prototype.
   * @param {unknown} value
   * @returns {value is Record<string, unknown>}
   */
  function isPlainObject(value) {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
      return false;
    }
    const prototype = Object.getPrototypeOf(value);
    return prototype === Object.prototype || prototype === null;
  }

  /**
   * @param {Record<string, unknown>} object
   * @param {readonly string[]} allowed
   */
  function hasOnlyKeys(object, allowed) {
    return Object.keys(object).every(
      (key) => !RESERVED_KEYS.includes(key) && allowed.includes(key),
    );
  }

  /** @param {Record<string, unknown>} object @param {string} key */
  function own(object, key) {
    return hasOwn(object, key) ? object[key] : undefined;
  }

  /** @param {unknown} value */
  function isCallbackId(value) {
    return Number.isInteger(value) && Number(value) >= 0 && Number(value) <= MAX_CALLBACK_ID;
  }

  /** @param {string} text */
  function decodedLength(text) {
    const padding = text.endsWith('==') ? 2 : text.endsWith('=') ? 1 : 0;
    return (text.length / 4) * 3 - padding;
  }

  /**
   * @param {unknown} value
   * @param {any} schema - one field of the allowlist
   * @returns {boolean}
   */
  function checkValue(value, schema) {
    if (!isPlainObject(schema)) {
      return false;
    }
    switch (schema.type) {
      case 'string':
        return typeof value === 'string' && value.length <= schema.maxLength;
      case 'base64':
        return (
          typeof value === 'string' &&
          value.length <= schema.maxChars &&
          value.length % 4 === 0 &&
          BASE64.test(value) &&
          decodedLength(value) <= schema.maxBytes
        );
      case 'id':
        return (
          typeof value === 'string' &&
          value.length === schema.prefix.length + schema.hexLength &&
          value.startsWith(schema.prefix) &&
          HEX.test(value.slice(schema.prefix.length))
        );
      case 'enum':
        return typeof value === 'string' && schema.values.includes(value);
      case 'int':
        return (
          Number.isInteger(value) && Number(value) >= schema.min && Number(value) <= schema.max
        );
      case 'intOneOf':
        return Number.isInteger(value) && schema.values.includes(value);
      case 'bool':
        return typeof value === 'boolean';
      case 'object':
        return checkObject(value, schema.fields);
      case 'channel':
        return typeof value === 'string' && CHANNEL.test(value);
      default:
        return false;
    }
  }

  /**
   * Exact keys: every key of `value` is a field, every required field is present, and every
   * present field holds a valid value (`null` never stands for an absent optional field).
   * @param {unknown} value
   * @param {unknown} fields
   * @returns {boolean}
   */
  function checkObject(value, fields) {
    if (!isPlainObject(value) || !isPlainObject(fields)) {
      return false;
    }
    const names = Object.keys(fields);
    if (!hasOnlyKeys(value, names)) {
      return false;
    }
    return names.every((name) => {
      const schema = fields[name];
      if (!hasOwn(value, name)) {
        return isPlainObject(schema) && schema.optional === true;
      }
      return checkValue(value[name], schema);
    });
  }

  /** The options of a channel fetch: none, or exactly the channel-ID header. @param {unknown} options */
  function isFetchOptions(options) {
    if (options === undefined) {
      return true;
    }
    if (!isPlainObject(options) || !hasOnlyKeys(options, ['headers'])) {
      return false;
    }
    const headers = own(options, 'headers');
    if (!isPlainObject(headers) || !hasOnlyKeys(headers, [FETCH_HEADER])) {
      return false;
    }
    const id = own(headers, FETCH_HEADER);
    return typeof id === 'string' && DIGITS.test(id);
  }

  if (!isPlainObject(message) || !hasOnlyKeys(message, MESSAGE_KEYS) || !isPlainObject(allowlist)) {
    return false;
  }
  const command = own(message, 'cmd');
  const payload = own(message, 'payload');
  const options = own(message, 'options');
  if (
    typeof command !== 'string' ||
    !isCallbackId(own(message, 'callback')) ||
    !isCallbackId(own(message, 'error'))
  ) {
    return false;
  }
  if (command === FETCH_COMMAND) {
    return payload === null && isFetchOptions(options);
  }
  if (options !== undefined || !hasOwn(allowlist, command)) {
    return false;
  }
  return checkObject(payload, allowlist[command]);
}
