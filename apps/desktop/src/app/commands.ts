/**
 * The command registry: every user action that a button, a shortcut or a menu can trigger has a
 * fixed ID, and the feature that implements it registers a handler (docs/spec/04-user-interface.md
 * §4.7). The toolbar and the shortcuts only ever run commands, so they do not depend on the
 * features.
 *
 * Registrations stack: the newest handler for an ID runs, and disposing it brings the previous one
 * back. The shell registers defaults for a few commands, which features may override.
 */
import { useSyncExternalStore } from 'react';

/** Every command the M2 editor has. */
export type CommandId =
  | 'project.new'
  | 'project.open'
  | 'project.save'
  | 'project.saveAs'
  | 'project.close'
  | 'build.start'
  | 'run.start'
  | 'run.stop'
  | 'run.again'
  | 'settings.open'
  | 'problems.focusFirstError'
  | 'edit.copy'
  | 'edit.cut'
  | 'edit.paste';

/** What a command does. It may be asynchronous; it reports its own errors to the user. */
export type CommandHandler = () => void | Promise<void>;

/** The registry behind the module's functions; tests and the feature context use it directly. */
export interface CommandRegistry {
  /** Registers `handler` for `id` and returns the function that removes this registration. */
  registerCommand(id: CommandId, handler: CommandHandler): () => void;
  /**
   * Runs the command's newest handler. While it is still running, running the same command again
   * returns the same promise instead of starting it twice (a double click or a held key). A
   * command with no handler does nothing. The promise rejects when the handler throws.
   */
  runCommand(id: CommandId): Promise<void>;
  /** Whether a handler is registered for `id`. */
  hasCommand(id: CommandId): boolean;
  /** Calls `listener` whenever a registration is added or removed. Returns the unsubscriber. */
  subscribe: (listener: () => void) => () => void;
}

/** `reason` as an `Error`, keeping a thrown non-error value as the cause. */
function asError(reason: unknown): Error {
  return reason instanceof Error ? reason : new Error('command failed', { cause: reason });
}

/** Creates an empty registry. */
export function createCommandRegistry(): CommandRegistry {
  const handlers = new Map<CommandId, CommandHandler[]>();
  const running = new Map<CommandId, Promise<void>>();
  const listeners = new Set<() => void>();

  function notify(): void {
    for (const listener of [...listeners]) {
      listener();
    }
  }

  return {
    registerCommand(id, handler) {
      // A wrapper gives each registration its own identity, even for the same function.
      const entry: CommandHandler = () => handler();
      handlers.set(id, [...(handlers.get(id) ?? []), entry]);
      notify();
      let registered = true;
      return () => {
        if (!registered) {
          return;
        }
        registered = false;
        const remaining = (handlers.get(id) ?? []).filter((candidate) => candidate !== entry);
        if (remaining.length === 0) {
          handlers.delete(id);
        } else {
          handlers.set(id, remaining);
        }
        notify();
      };
    },

    runCommand(id) {
      const pending = running.get(id);
      if (pending !== undefined) {
        return pending;
      }
      const handler = handlers.get(id)?.at(-1);
      if (handler === undefined) {
        return Promise.resolve();
      }
      // The handler starts synchronously, so it can still use the event that triggered it (for
      // example the clipboard data of a copy event).
      let result: ReturnType<CommandHandler>;
      try {
        result = handler();
      } catch (error: unknown) {
        return Promise.reject(asError(error));
      }
      if (!(result instanceof Promise)) {
        return Promise.resolve();
      }
      const tracked = result.finally(() => {
        running.delete(id);
      });
      running.set(id, tracked);
      return tracked;
    },

    hasCommand(id) {
      return handlers.has(id);
    },

    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The app's command registry. */
export const commands: CommandRegistry = createCommandRegistry();

/** Registers a handler with the app's registry; see {@link CommandRegistry.registerCommand}. */
export function registerCommand(id: CommandId, handler: CommandHandler): () => void {
  return commands.registerCommand(id, handler);
}

/** Runs a command of the app's registry; see {@link CommandRegistry.runCommand}. */
export function runCommand(id: CommandId): Promise<void> {
  return commands.runCommand(id);
}

/**
 * Runs a command from a user-interface event (a click or a key press), where nobody awaits the
 * result: a failure is logged instead of becoming an unhandled rejection. Handlers show their own
 * messages to the user; the log keeps only the command ID and the error.
 */
export function triggerCommand(id: CommandId, registry: CommandRegistry = commands): void {
  registry.runCommand(id).catch((error: unknown) => {
    console.error(`Command ${id} failed`, error);
  });
}

/** Whether `id` has a handler, as React state that follows registrations. */
export function useCommandAvailable(id: CommandId, registry: CommandRegistry = commands): boolean {
  return useSyncExternalStore(registry.subscribe, () => registry.hasCommand(id));
}
