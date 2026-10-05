/**
 * Wraps the raw glue functions of one WebAssembly instance as a `CoreWasm`: JSON in and out,
 * error envelopes turned into `CoreError`, traps into `CoreTrap`.
 */

import type { Glue } from '#glue';

import type {
  CanonicalResult,
  CoreVersion,
  CoreWasm,
  LoadResult,
  PreviewOptions,
  PreviewResult,
} from './api';
import { CoreError, type CoreErrorKind, CoreTrap } from './errors';

/**
 * The largest project the loader accepts, in bytes (05 §5.6, `b2c_model::limits::MAX_FILE_BYTES`).
 * Larger input is cut to one byte over the limit before it enters the WebAssembly memory, so the
 * loader still reports it (`B2C-E0101`) while the instance's memory stays bounded.
 */
export const MAX_DOCUMENT_BYTES = 32 * 1024 * 1024;

/** The glue functions a core needs (everything but `init`). */
export type GlueFunctions = Pick<Glue, 'version' | 'load' | 'canonical' | 'preview'>;

/** A core and the state the loader needs to know about it. */
export interface CoreHandle {
  readonly core: CoreWasm;
  /** Whether the instance has stopped; a stopped instance is never called again. */
  readonly trapped: boolean;
}

const KINDS_FROM_CORE: readonly CoreErrorKind[] = ['invalidOptions', 'encode'];

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Turns a JSON response into a result, or throws the `CoreError` of an error envelope
 * (`{"error": {"kind", "message"}}`).
 */
function parseResponse(operation: string, text: string): object {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new CoreError('protocol', `${operation}: the core returned text that is not JSON`, {
      cause: error,
    });
  }
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new CoreError(
      'protocol',
      `${operation}: the core returned something other than an object`,
    );
  }
  if (Object.hasOwn(value, 'error')) {
    const envelope = (value as { error: unknown }).error;
    const fields =
      typeof envelope === 'object' && envelope !== null
        ? (envelope as { kind?: unknown; message?: unknown })
        : {};
    const kind = KINDS_FROM_CORE.find((known) => known === fields.kind) ?? 'protocol';
    const message = typeof fields.message === 'string' ? fields.message : 'unknown error';
    throw new CoreError(kind, `${operation}: ${message}`);
  }
  return value;
}

/** Wraps one instance's glue functions. */
export function createCore(glue: GlueFunctions): CoreHandle {
  let trapped = false;

  function call(operation: string, run: () => string): object {
    if (trapped) {
      throw new CoreTrap(
        `${operation}: the compiler core stopped after an earlier internal error; start a new one with initCore()`,
      );
    }
    let text: string;
    try {
      text = run();
    } catch (error) {
      trapped = true;
      throw new CoreTrap(`${operation}: the compiler core stopped: ${describe(error)}`, {
        cause: error,
      });
    }
    return parseResponse(operation, text);
  }

  const core: CoreWasm = {
    version: () => call('version', () => glue.version()) as CoreVersion,
    load: (bytes) => {
      const bounded =
        bytes.length > MAX_DOCUMENT_BYTES ? bytes.subarray(0, MAX_DOCUMENT_BYTES + 1) : bytes;
      return call('load', () => glue.load(bounded)) as LoadResult;
    },
    canonical: (documentJson) => {
      const bounded = boundText(documentJson);
      return call('canonical', () => glue.canonical(bounded)) as CanonicalResult;
    },
    preview: (documentJson, options: PreviewOptions) => {
      const bounded = boundText(documentJson);
      // Only the known key crosses, whatever else the object carries.
      const optionsJson = JSON.stringify({ indentWidth: options.indentWidth });
      return call('preview', () => glue.preview(bounded, optionsJson)) as PreviewResult;
    },
  };

  return {
    core,
    get trapped() {
      return trapped;
    },
  };
}

/**
 * Cuts text that is certainly over the size limit (its UTF-8 form is at least as long as its
 * UTF-16 length) to one code unit over it.
 */
function boundText(text: string): string {
  return text.length > MAX_DOCUMENT_BYTES ? text.slice(0, MAX_DOCUMENT_BYTES + 1) : text;
}
