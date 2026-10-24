/**
 * Checks for the messages of the build and run channels (docs/spec/02-architecture.md §2.5.3).
 *
 * The backend ships with the frontend, but a channel message is input like any other: each one is
 * checked against the shape the contract gives before it is used, and a message that does not fit
 * is ignored (and logged without its content). Diagnostics that do not fit are dropped one by one.
 */
import type {
  BuildEvent,
  BuildOutcome,
  BuildStage,
  Containment,
  Crash,
  Diagnostic,
  DiagSource,
  ExitStatus,
  RunEvent,
  RunMode,
  Severity,
} from '@blocks2cpp/ipc-types';

/** The most diagnostics one `diagnostics` build event may carry; the rest are dropped. */
export const MAX_DIAGNOSTICS_PER_EVENT = 10_000;

/** The most related locations kept per diagnostic. */
export const MAX_RELATED_PER_DIAGNOSTIC = 64;

const BUILD_STAGES: ReadonlySet<string> = new Set<BuildStage>(['generate', 'compile', 'link']);
const BUILD_OUTCOMES: ReadonlySet<string> = new Set<BuildOutcome>([
  'built',
  'upToDate',
  'projectErrors',
  'toolchainProblem',
  'cancelled',
  'failed',
]);
const CONTAINMENTS: ReadonlySet<string> = new Set<Containment>([
  'jobObject',
  'cgroup',
  'processGroupOnly',
]);
const RUN_MODES: ReadonlySet<string> = new Set<RunMode>(['pty', 'pipes']);
const CRASHES: ReadonlySet<string> = new Set<Crash>([
  'memoryAccess',
  'stackOverflow',
  'divisionByZero',
  'aborted',
  'trap',
  'interrupted',
  'terminated',
  'killed',
  'outOfMemory',
  'heapCorruption',
  'missingDll',
  'brokenPipe',
  'resourceLimit',
  'other',
]);
const SEVERITIES: ReadonlySet<string> = new Set<Severity>(['info', 'warning', 'error']);
const SOURCES: ReadonlySet<string> = new Set<DiagSource>([
  'loader',
  'catalog',
  'analyser',
  'generator',
  'toolchain',
  'compiler',
  'linker',
  'runtime',
]);
const PART_KINDS: ReadonlySet<string> = new Set(['whole', 'field', 'input', 'tokens']);

/** A content hash: 64 lower-case hex digits (05 §5.11). */
const HASH = /^[0-9a-f]{64}$/;

/** A sanitizer kind: 1–64 characters from `a`–`z`, `0`–`9` and `-` (02 §2.5.3). */
const SANITIZER_KIND = /^[a-z0-9-]{1,64}$/;

type Fields = Record<string, unknown>;

function isObject(value: unknown): value is Fields {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** A non-negative safe integer, as the contract's counters and sequence numbers are. */
function isCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function isOneOf(set: ReadonlySet<string>, value: unknown): value is string {
  return typeof value === 'string' && set.has(value);
}

function isOptionalString(value: unknown): boolean {
  return value === undefined || typeof value === 'string';
}

function isPart(value: unknown): boolean {
  if (!isObject(value) || !isOneOf(PART_KINDS, value['kind'])) {
    return false;
  }
  switch (value['kind']) {
    case 'field':
    case 'input':
      return typeof value['name'] === 'string';
    case 'tokens':
      return typeof value['input'] === 'string' && isCount(value['start']) && isCount(value['end']);
    default:
      return true;
  }
}

function isLocation(value: unknown): boolean {
  return (
    isObject(value) &&
    isOptionalString(value['module']) &&
    isOptionalString(value['block']) &&
    isPart(value['part'])
  );
}

/**
 * The diagnostic in `value` (a new object with only the contract's keys), or `null` when it does
 * not have the contract's shape. Related locations that do not fit are dropped, and at most
 * {@link MAX_RELATED_PER_DIAGNOSTIC} are kept.
 */
export function checkDiagnostic(value: unknown): Diagnostic | null {
  if (
    !isObject(value) ||
    typeof value['code'] !== 'string' ||
    !isOneOf(SEVERITIES, value['severity']) ||
    typeof value['message'] !== 'string' ||
    !isLocation(value['primary']) ||
    !isOneOf(SOURCES, value['source']) ||
    !isOptionalString(value['raw'])
  ) {
    return null;
  }
  const shaped = value as unknown as Diagnostic;
  const diagnostic: Diagnostic = {
    code: shaped.code,
    severity: shaped.severity,
    message: shaped.message,
    primary: shaped.primary,
    source: shaped.source,
  };
  if (shaped.raw !== undefined) {
    diagnostic.raw = shaped.raw;
  }
  const related: unknown = value['related'];
  if (Array.isArray(related)) {
    const kept = (related as unknown[])
      .filter(
        (entry) =>
          isObject(entry) && isLocation(entry['location']) && typeof entry['message'] === 'string',
      )
      .slice(0, MAX_RELATED_PER_DIAGNOSTIC) as NonNullable<Diagnostic['related']>;
    if (kept.length > 0) {
      diagnostic.related = kept;
    }
  }
  return diagnostic;
}

/**
 * The build event in `message`, or `null` when it is not one. The diagnostics of a `diagnostics`
 * event are checked one by one; those that do not fit are dropped, and at most
 * {@link MAX_DIAGNOSTICS_PER_EVENT} are kept.
 */
export function checkBuildEvent(message: unknown): BuildEvent | null {
  if (!isObject(message)) {
    return null;
  }
  switch (message['kind']) {
    case 'progress':
      return isOneOf(BUILD_STAGES, message['stage']) &&
        isCount(message['done']) &&
        isCount(message['total'])
        ? (message as unknown as BuildEvent)
        : null;
    case 'diagnostics': {
      const items: unknown = message['items'];
      if (!Array.isArray(items)) {
        return null;
      }
      const checked: Diagnostic[] = [];
      for (const item of items as unknown[]) {
        if (checked.length >= MAX_DIAGNOSTICS_PER_EVENT) {
          break;
        }
        const diagnostic = checkDiagnostic(item);
        if (diagnostic !== null) {
          checked.push(diagnostic);
        }
      }
      return { kind: 'diagnostics', items: checked };
    }
    case 'finished': {
      const hash: unknown = message['projectHash'];
      return isOneOf(BUILD_OUTCOMES, message['outcome']) &&
        (hash === null || (typeof hash === 'string' && HASH.test(hash))) &&
        isCount(message['elapsedMs'])
        ? (message as unknown as BuildEvent)
        : null;
    }
    default:
      return null;
  }
}

function isExitStatus(value: unknown): value is ExitStatus {
  if (!isObject(value)) {
    return false;
  }
  switch (value['type']) {
    case 'exited':
      return typeof value['code'] === 'number' && Number.isSafeInteger(value['code']);
    case 'signaled':
      return isCount(value['signal']);
    case 'exception':
      return isCount(value['ntstatus']);
    case 'stopped':
      return true;
    default:
      return false;
  }
}

function isSanitizer(value: unknown): boolean {
  return (
    value === null ||
    (isObject(value) &&
      (value['tool'] === 'address' || value['tool'] === 'undefined') &&
      typeof value['kind'] === 'string' &&
      SANITIZER_KIND.test(value['kind']))
  );
}

/** The run event in `message`, or `null` when it is not one. */
export function checkRunEvent(message: unknown): RunEvent | null {
  if (!isObject(message)) {
    return null;
  }
  let valid: boolean;
  switch (message['kind']) {
    case 'started':
      valid =
        isOneOf(CONTAINMENTS, message['containment']) &&
        isOneOf(RUN_MODES, message['mode']) &&
        typeof message['ideHelpers'] === 'boolean';
      break;
    case 'skipped':
      valid = isCount(message['lines']) && isCount(message['afterSeq']);
      break;
    case 'exit':
      valid =
        isCount(message['afterSeq']) &&
        isCount(message['elapsedMs']) &&
        isExitStatus(message['status']) &&
        (message['crash'] === null || isOneOf(CRASHES, message['crash'])) &&
        isSanitizer(message['sanitizer']) &&
        typeof message['message'] === 'string';
      break;
    default:
      valid = false;
  }
  return valid ? (message as unknown as RunEvent) : null;
}
