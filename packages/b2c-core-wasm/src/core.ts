/**
 * Wraps the raw glue functions of one WebAssembly instance as a `CoreWasm`: JSON in and out,
 * error envelopes turned into `CoreError`, traps into `CoreTrap`.
 */

import type { SymbolInfo } from '@blocks2cpp/ipc-types';

import type { Glue } from '#glue';

import type {
  CanonicalResult,
  ClipboardMakeResult,
  ConversionRow,
  CoreVersion,
  CoreWasm,
  LoadResult,
  PastePrepareResult,
  PreviewOptions,
  PreviewResult,
} from './api';
import { CoreError, type CoreErrorKind, CoreTrap } from './errors';

/**
 * The largest project the loader accepts, in bytes (05 §5.6, `b2c_model::limits::MAX_FILE_BYTES`).
 * Larger input is cut to one byte over the limit before it enters the WebAssembly memory, so the
 * loader still reports it (`B2C-E0101`) while the instance's memory stays bounded. Clipboard
 * payloads have the same limit, and every other text argument is bounded the same way (the core
 * applies its own, smaller limits to them).
 */
export const MAX_DOCUMENT_BYTES = 32 * 1024 * 1024;

/** The glue functions a core needs (everything but `init`). */
export type GlueFunctions = Omit<Glue, 'init'>;

/** A core and the state the loader needs to know about it. */
export interface CoreHandle {
  readonly core: CoreWasm;
  /** Whether the instance has stopped; a stopped instance is never called again. */
  readonly trapped: boolean;
}

const KINDS_FROM_CORE: readonly CoreErrorKind[] = [
  'invalidOptions',
  'invalidArguments',
  'encode',
  'internal',
];

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Parses a response, or throws `CoreError` with kind `protocol` for text that is not JSON. */
function parseJson(operation: string, text: string): unknown {
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new CoreError('protocol', `${operation}: the core returned text that is not JSON`, {
      cause: error,
    });
  }
}

/** Throws the `CoreError` of an error envelope (`{"error": {"kind", "message"}}`), if it is one. */
function throwIfEnvelope(operation: string, value: object): void {
  if (Array.isArray(value) || !Object.hasOwn(value, 'error')) {
    return;
  }
  const envelope = (value as { error: unknown }).error;
  const fields =
    typeof envelope === 'object' && envelope !== null
      ? (envelope as { kind?: unknown; message?: unknown })
      : {};
  const kind = KINDS_FROM_CORE.find((known) => known === fields.kind) ?? 'protocol';
  const message = typeof fields.message === 'string' ? fields.message : 'unknown error';
  throw new CoreError(kind, `${operation}: ${message}`);
}

/** A JSON object response, or the `CoreError` of an error envelope. */
function parseObject(operation: string, text: string): object {
  const value = parseJson(operation, text);
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new CoreError(
      'protocol',
      `${operation}: the core returned something other than an object`,
    );
  }
  throwIfEnvelope(operation, value);
  return value;
}

/** A JSON list response, or the `CoreError` of an error envelope. */
function parseList(operation: string, text: string): unknown[] {
  const value = parseJson(operation, text);
  if (typeof value === 'object' && value !== null) {
    throwIfEnvelope(operation, value);
  }
  if (!Array.isArray(value)) {
    throw new CoreError('protocol', `${operation}: the core returned something other than a list`);
  }
  return value;
}

/**
 * Cuts text that is certainly over the size limit (its UTF-8 form is at least as long as its
 * UTF-16 length) to one code unit over it.
 */
function boundText(text: string): string {
  return text.length > MAX_DOCUMENT_BYTES ? text.slice(0, MAX_DOCUMENT_BYTES + 1) : text;
}

/** Wraps one instance's glue functions. */
export function createCore(glue: GlueFunctions): CoreHandle {
  let trapped = false;

  function call(operation: string, run: () => string): string {
    if (trapped) {
      throw new CoreTrap(
        `${operation}: the compiler core stopped after an earlier internal error; start a new one with initCore()`,
      );
    }
    try {
      return run();
    } catch (error) {
      trapped = true;
      throw new CoreTrap(`${operation}: the compiler core stopped: ${describe(error)}`, {
        cause: error,
      });
    }
  }

  const object = (operation: string, run: () => string): object =>
    parseObject(operation, call(operation, run));
  const list = (operation: string, run: () => string): unknown[] =>
    parseList(operation, call(operation, run));

  const core: CoreWasm = {
    version: () => object('version', () => glue.version()) as CoreVersion,
    load: (bytes) => {
      const bounded =
        bytes.length > MAX_DOCUMENT_BYTES ? bytes.subarray(0, MAX_DOCUMENT_BYTES + 1) : bytes;
      return object('load', () => glue.load(bounded)) as LoadResult;
    },
    canonical: (documentJson) => {
      const bounded = boundText(documentJson);
      return object('canonical', () => glue.canonical(bounded)) as CanonicalResult;
    },
    preview: (documentJson, options: PreviewOptions) => {
      const bounded = boundText(documentJson);
      // Only the known key crosses, whatever else the object carries.
      const optionsJson = JSON.stringify({ indentWidth: options.indentWidth });
      return object('preview', () => glue.preview(bounded, optionsJson)) as PreviewResult;
    },
    symbolsInScope: (blockId, input) => {
      const block = boundText(blockId);
      const name = input === null ? null : boundText(input);
      return list('symbolsInScope', () => glue.symbols_in_scope(block, name)) as SymbolInfo[];
    },
    conversionTable: () =>
      list('conversionTable', () => glue.conversion_table()) as ConversionRow[],
    clipboardMake: (documentJson, blockIds) => {
      const bounded = boundText(documentJson);
      const idsJson = boundText(JSON.stringify(blockIds));
      return object('clipboardMake', () =>
        glue.clipboard_make(bounded, idsJson),
      ) as ClipboardMakeResult;
    },
    pastePrepare: (clipboardText, documentJson, target, seedHex) => {
      const payload = boundText(clipboardText);
      const bounded = boundText(documentJson);
      // Only the known keys cross, whatever else the object carries.
      const targetJson = boundText(
        JSON.stringify({ module: target.module, block: target.block, input: target.input }),
      );
      const seed = boundText(seedHex);
      return object('pastePrepare', () =>
        glue.paste_prepare(payload, bounded, targetJson, seed),
      ) as PastePrepareResult;
    },
  };

  return {
    core,
    get trapped() {
      return trapped;
    },
  };
}
