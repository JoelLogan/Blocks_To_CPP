// Types of build-info.mjs (the fingerprint of the WebAssembly core's inputs), for the desktop
// app's Vite plugin and the tests. Keep in step with the module.

/** The core package's folder, relative to the repository root. */
export const CORE_PACKAGE_DIR: string;
/** The build's output folder, relative to the repository root. */
export const PKG_DIR: string;
/** The file in pkg/ that records what the build was made from. */
export const BUILD_INFO_FILE: string;
/** The layout of build-info.json. */
export const BUILD_INFO_VERSION: number;
/** The crates compiled into the module. */
export const CORE_CRATES: readonly string[];
/** The command that rebuilds the core, from the repository root. */
export const REBUILD_COMMAND: string;

/** A named group of inputs: files or folders relative to the repository root. */
export interface InputGroup {
    readonly name: string;
    readonly paths: readonly string[];
}

/** The inputs, in a fixed order; the `Cargo.lock` group has no paths (it is a closure). */
export const INPUT_GROUPS: readonly InputGroup[];

/** An input that cannot be read, or a Cargo.lock the module does not understand. */
export class BuildInfoError extends Error {}

/** One `[[package]]` entry of a Cargo.lock. */
export interface LockEntry {
    name: string;
    version: string;
    /** As written in the lockfile: `name`, `name version` or `name version (source)`. */
    dependencies: string[];
    /** The entry's text. */
    entry: string;
}

/** The fingerprint of the inputs: one SHA-256 per group and one over all of them (hex). */
export interface Fingerprint {
    fingerprint: string;
    inputs: Record<string, string>;
    /** How many files were read (the Cargo.lock group counts as one). */
    files: number;
}

/** What a build was made with, besides its inputs. */
export interface BuildTools {
    /** The wasm-bindgen CLI's version. */
    wasmBindgen: string;
    /** wasm-opt's `--version` output, or `null` when the build skipped it. */
    wasmOpt: string | null;
}

/** Whether the built core matches the source (see `checkBuild`). */
export type BuildState =
    | { state: 'fresh' }
    | { state: 'stale'; changed: string[] }
    | { state: 'notBuilt' }
    | { state: 'unknown'; reason: 'missing' | 'unreadable' };

export function parseLock(text: string): LockEntry[];
export function lockClosure(text: string, root: string): LockEntry[];
export function computeFingerprint(repoRoot: string): Fingerprint;
export function buildInfoText(fingerprint: Fingerprint, tools: BuildTools): string;
export function checkBuild(repoRoot: string): BuildState;
export function describeBuild(result: BuildState): string | null;
