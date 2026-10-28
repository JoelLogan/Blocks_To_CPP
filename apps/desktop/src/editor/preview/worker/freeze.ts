/**
 * The preview worker's first import (ADR-0016 §6): freezes `Object.prototype` before the core's
 * glue and the host run, as Tauri's `freezePrototype` does for the page's realm, which does not
 * reach a worker.
 */
Object.freeze(Object.prototype);
