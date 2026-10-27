/**
 * `@blocks2cpp/b2c-core-wasm/worker`: the preview worker's protocol, its client (the page) and its
 * host (the worker), ADR-0016. Nothing here imports the embedded bytes (`#pkg-bytes`), so the
 * worker's own bundle never holds them: the page compiles the module with `coreModule()` from the
 * package's main entry and posts it.
 */

export {
  type CoreWorkerClient,
  type CoreWorkerClientOptions,
  type CoreWorkerState,
  createCoreWorkerClient,
  DEFAULT_MAX_FAILED_STARTS,
  DEFAULT_START_TIMEOUT_MS,
  DEFAULT_UPDATE_TIMEOUT_MS,
  type WorkerLike,
  type WorkerUpdate,
} from './client';
export { serveCoreWorker, type ServeOptions, type WorkerScopeLike } from './host';
export {
  MAX_REPLY_MESSAGE,
  parseReply,
  parseRequest,
  WORKER_PROTOCOL,
  type WorkerProbe,
  type WorkerReply,
  type WorkerRequest,
} from './protocol';
