/**
 * The preview worker's message protocol (ADR-0016 §3, docs/plans/m3-contracts/worker.md §3): what
 * the page and the worker post each other, and the checks each side applies to what it receives.
 * Both sides ship in one build, so a message that fails a check is a bug or a misbehaving worker,
 * never something to adapt to: the worker logs and ignores it, the client replaces the worker.
 *
 * Messages carry no project text but the document itself and the core's answer about it, and
 * neither is ever logged.
 */

/** The protocol version every message carries. */
export const WORKER_PROTOCOL = 1;

/** What the end-to-end builds' probe found in the worker (ADR-0016 §6). */
export interface WorkerProbe {
  /** Whether `eval('1')` ran. */
  readonly eval: 'blocked' | 'allowed';
  /**
   * Whether a request to the IPC endpoint was refused by the policy (`blocked`), answered
   * (`allowed`), or failed for another reason (`failed`).
   */
  readonly request: 'blocked' | 'allowed' | 'failed';
  /** The violated directives of the worker's `securitypolicyviolation` events, each once, sorted. */
  readonly directives: readonly string[];
}

/** Page → worker. */
export type WorkerRequest =
  | { readonly p: 1; readonly kind: 'start'; readonly module: WebAssembly.Module }
  | {
      readonly p: 1;
      readonly kind: 'update';
      readonly id: number;
      readonly documentJson: string;
      readonly optionsJson: string;
    }
  /** End-to-end builds only. */
  | { readonly p: 1; readonly kind: 'probe'; readonly id: number };

/** Worker → page. */
export type WorkerReply =
  | { readonly p: 1; readonly kind: 'started'; readonly versionJson: string }
  | { readonly p: 1; readonly kind: 'startFailed'; readonly message: string }
  | {
      readonly p: 1;
      readonly kind: 'updated';
      readonly id: number;
      /** What `update()` returned, unparsed: a result or an error envelope. */
      readonly updateJson: string;
      /** What `scope_index()` returned, or null when the document did not load (or no index yet). */
      readonly scopeJson: string | null;
      /** When `update()` started: `performance.timeOrigin + performance.now()` in the worker. */
      readonly startedAt: number;
      /** How long `update()` alone took, in milliseconds (N4's metric). */
      readonly updateMs: number;
      /** How long `scope_index()` took, in milliseconds. */
      readonly indexMs: number;
    }
  | { readonly p: 1; readonly kind: 'trapped'; readonly id: number; readonly message: string }
  | { readonly p: 1; readonly kind: 'probed'; readonly id: number; readonly probe: WorkerProbe };

/** The longest `message` of a `startFailed` or `trapped` reply the page keeps. */
export const MAX_REPLY_MESSAGE = 512;

/** The most directives a probe may report. */
const MAX_PROBE_DIRECTIVES = 32;

/** A directive's name, as `src/lib/trustedTypes.ts` of the app accepts it. */
const DIRECTIVE_NAME = /^[a-z][a-z-]{0,63}$/;

type Fields = Readonly<Record<string, unknown>>;

/**
 * The fields of a structured-cloned plain object carrying `p: 1` and exactly `keys`, or `null`.
 * A clone of a plain object has `Object.prototype` (or none, for one made so) and own data
 * properties only; anything else did not come from the other side.
 */
function fieldsOf(data: unknown, keys: readonly string[]): Fields | null {
  if (typeof data !== 'object' || data === null || Array.isArray(data)) {
    return null;
  }
  const prototype: unknown = Object.getPrototypeOf(data);
  if (prototype !== Object.prototype && prototype !== null) {
    return null;
  }
  const own = Object.keys(data);
  if (own.length !== keys.length || !keys.every((key) => Object.hasOwn(data, key))) {
    return null;
  }
  const fields = data as Fields;
  return fields['p'] === WORKER_PROTOCOL ? fields : null;
}

/** The message's `kind`, when it is a string. */
function kindOf(data: unknown): string | null {
  if (typeof data !== 'object' || data === null || !Object.hasOwn(data, 'kind')) {
    return null;
  }
  const kind = (data as Fields)['kind'];
  return typeof kind === 'string' ? kind : null;
}

/** A request ID: a positive safe integer. */
function isId(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
}

/** A duration or time stamp: a finite number, never negative. */
function isTime(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function isMessage(value: unknown): value is string {
  return typeof value === 'string' && value.length <= MAX_REPLY_MESSAGE;
}

function isProbe(value: unknown): value is WorkerProbe {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const keys = Object.keys(value);
  if (keys.length !== 3) {
    return false;
  }
  const { eval: evaluated, request, directives } = value as Fields;
  return (
    (evaluated === 'blocked' || evaluated === 'allowed') &&
    (request === 'blocked' || request === 'allowed' || request === 'failed') &&
    Array.isArray(directives) &&
    directives.length <= MAX_PROBE_DIRECTIVES &&
    directives.every((directive) => typeof directive === 'string' && DIRECTIVE_NAME.test(directive))
  );
}

/**
 * The request `data` is, or `null` when it is not one of this protocol: another version, an
 * unknown kind, a missing, extra or mistyped field.
 */
export function parseRequest(data: unknown): WorkerRequest | null {
  switch (kindOf(data)) {
    case 'start': {
      const fields = fieldsOf(data, ['p', 'kind', 'module']);
      return fields !== null && fields['module'] instanceof WebAssembly.Module
        ? (fields as WorkerRequest)
        : null;
    }
    case 'update': {
      const fields = fieldsOf(data, ['p', 'kind', 'id', 'documentJson', 'optionsJson']);
      return fields !== null &&
        isId(fields['id']) &&
        typeof fields['documentJson'] === 'string' &&
        typeof fields['optionsJson'] === 'string'
        ? (fields as WorkerRequest)
        : null;
    }
    case 'probe': {
      const fields = fieldsOf(data, ['p', 'kind', 'id']);
      return fields !== null && isId(fields['id']) ? (fields as WorkerRequest) : null;
    }
    default:
      return null;
  }
}

/**
 * The reply `data` is, or `null` when it is not one of this protocol: another version, an unknown
 * kind, a missing, extra or mistyped field.
 */
export function parseReply(data: unknown): WorkerReply | null {
  switch (kindOf(data)) {
    case 'started': {
      const fields = fieldsOf(data, ['p', 'kind', 'versionJson']);
      return fields !== null && typeof fields['versionJson'] === 'string'
        ? (fields as WorkerReply)
        : null;
    }
    case 'startFailed': {
      const fields = fieldsOf(data, ['p', 'kind', 'message']);
      return fields !== null && isMessage(fields['message']) ? (fields as WorkerReply) : null;
    }
    case 'updated': {
      const fields = fieldsOf(data, [
        'p',
        'kind',
        'id',
        'updateJson',
        'scopeJson',
        'startedAt',
        'updateMs',
        'indexMs',
      ]);
      return fields !== null &&
        isId(fields['id']) &&
        typeof fields['updateJson'] === 'string' &&
        (fields['scopeJson'] === null || typeof fields['scopeJson'] === 'string') &&
        isTime(fields['startedAt']) &&
        isTime(fields['updateMs']) &&
        isTime(fields['indexMs'])
        ? (fields as WorkerReply)
        : null;
    }
    case 'trapped': {
      const fields = fieldsOf(data, ['p', 'kind', 'id', 'message']);
      return fields !== null && isId(fields['id']) && isMessage(fields['message'])
        ? (fields as WorkerReply)
        : null;
    }
    case 'probed': {
      const fields = fieldsOf(data, ['p', 'kind', 'id', 'probe']);
      return fields !== null && isId(fields['id']) && isProbe(fields['probe'])
        ? (fields as WorkerReply)
        : null;
    }
    default:
      return null;
  }
}
