/**
 * The typed client for the backend's IPC commands (docs/spec/02-architecture.md §2.5).
 *
 * Every command the editor calls goes through {@link ipc}. Its methods, request and response types
 * are generated from the Rust command table in `crates/b2c-ipc` (`@blocks2cpp/ipc-types`), so the
 * frontend cannot call a command the backend does not have or send a request of the wrong shape.
 * The backend permits each command in src-tauri/capabilities/, and the isolation hook in
 * src-tauri/isolation/ checks each message's arguments against the generated allowlist before the
 * backend sees it.
 */
import { Channel, invoke } from '@tauri-apps/api/core';
import { createIpcClient, type IpcClient, type IpcTransport } from '@blocks2cpp/ipc-types';

/**
 * Tauri's transport: `invoke` for calls, and a `Channel` per push-channel argument (app events,
 * build events, run output and run events). Channels are passed as command arguments, never as
 * global events: the window has no `core:event` permission (§2.5.3).
 */
export const tauriTransport: IpcTransport = {
  invoke: (cmd, args) => invoke(cmd, args),
  channel: (onMessage) => {
    const channel = new Channel<unknown>();
    channel.onmessage = onMessage;
    return channel;
  },
};

/** The client over Tauri, used unless a test replaces it. */
const tauriClient: IpcClient = createIpcClient(tauriTransport);

/** The client every call of {@link ipc} goes to. */
let active: IpcClient = tauriClient;

type AnyMethod = (...args: never[]) => unknown;

/**
 * A client whose every method forwards to `current()` at call time. Code that kept a reference to
 * {@link ipc} (for example a feature's context) therefore follows {@link setIpcForTests}.
 */
function forwardingClient(current: () => IpcClient): IpcClient {
  const methods: Record<string, AnyMethod> = {};
  for (const name of Object.keys(tauriClient) as (keyof IpcClient)[]) {
    methods[name] = (...args) => (current()[name] as AnyMethod)(...args);
  }
  return Object.freeze(methods) as unknown as IpcClient;
}

/**
 * The backend's commands, one method per command. Every method rejects with an `IpcCallError`
 * carrying the command's typed `IpcError` (or a transport failure); user-facing text for those
 * errors comes from the frontend, never from the backend.
 */
export const ipc: IpcClient = forwardingClient(() => active);

/**
 * Replaces the client behind {@link ipc}, for tests that script the backend. `null` restores the
 * Tauri client.
 */
export function setIpcForTests(client: IpcClient | null): void {
  active = client ?? tauriClient;
}
