/**
 * Small corrections that make happy-dom behave like the webviews the app runs in, where a test
 * depends on that behaviour. Each one copies what browsers do; none adds a feature browsers lack.
 *
 * A copy of apps/desktop/src/test/dom-shims.ts (packages may not depend on the app); keep the two
 * in step.
 */

const METHODS = ['addEventListener', 'removeEventListener'] as const;

type ListenerMethodName = (typeof METHODS)[number];
/** The arguments both methods take (their option types are compatible). */
type ListenerArgs = Parameters<EventTarget['addEventListener']>;
type ListenerMethod = (this: EventTarget, ...args: ListenerArgs) => void;

/** Marks the prototype whose listener methods are already shimmed, with the shimmed names. */
const SHIMMED = Symbol.for('blocks2cpp.test.eventTargetReceiverShim');

/**
 * In browsers, `addEventListener` and `removeEventListener` called without a receiver act on the
 * global object (Web IDL: a missing `this` means the global object). Blockly 12's focus manager
 * relies on that: it stores `document.addEventListener` and calls it unbound, so its listeners end
 * up on `window`. happy-dom throws instead, which breaks `Blockly.inject`.
 *
 * This shim gives happy-dom the browser behaviour: on the prototype that defines the two methods
 * for `document`, a call without a receiver is passed on to `window`. (Under Vitest, `window` is
 * Node's global object with happy-dom's window methods copied onto it, already bound.) It is
 * idempotent.
 */
export function installEventTargetReceiverShim(): void {
  for (const name of METHODS) {
    const owner = prototypeDefining(document, name);
    if (owner !== null) {
      shimReceiver(owner, name);
    }
  }
}

/** The object in `start`'s prototype chain (itself included) that has `name` as its own property. */
function prototypeDefining(start: object, name: string): object | null {
  for (let current: object | null = start; current !== null;) {
    if (Object.hasOwn(current, name)) {
      return current;
    }
    current = Object.getPrototypeOf(current) as object | null;
  }
  return null;
}

function shimReceiver(owner: object, name: ListenerMethodName): void {
  const marked = owner as { [SHIMMED]?: Set<string> };
  const done = marked[SHIMMED] ?? new Set<string>();
  if (done.has(name)) {
    return;
  }
  const original = (owner as Record<string, unknown>)[name] as ListenerMethod;
  const globalTarget: EventTarget = window;
  const onWindow =
    name === 'addEventListener'
      ? (...args: ListenerArgs) => {
          globalTarget.addEventListener(...args);
        }
      : (...args: ListenerArgs) => {
          globalTarget.removeEventListener(...args);
        };
  Object.defineProperty(owner, name, {
    configurable: true,
    writable: true,
    value: function (this: EventTarget | undefined, ...args: ListenerArgs): void {
      if (this === undefined) {
        onWindow(...args);
      } else {
        original.apply(this, args);
      }
    },
  });
  done.add(name);
  marked[SHIMMED] = done;
}
