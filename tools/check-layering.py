#!/usr/bin/env python3
"""Checks the crate layering rules of docs/spec/02-architecture.md §2.3.

Reads the workspace's dependency declarations with `cargo metadata` and fails
when a crate depends on a workspace crate it may not use, or when one of the
pure compiler crates (no I/O: no filesystem, processes, clock or randomness)
gains an external dependency that is not on its allowlist.

Run from the repository root: python3 tools/check-layering.py
"""

import json
import subprocess
import sys

# Workspace crates each crate may use in normal and build dependencies.
ALLOWED_WORKSPACE_DEPS = {
    "b2c-ir": set(),
    "b2c-model": {"b2c-ir"},
    "b2c-catalog": {"b2c-ir", "b2c-model"},
    "b2c-lang": {"b2c-ir", "b2c-model"},
    "b2c-codegen": {"b2c-ir"},
    "b2c-core-wasm": {"b2c-ir", "b2c-model", "b2c-catalog", "b2c-lang", "b2c-codegen"},
    "b2c-process": set(),
    # Machine-local storage (02 §2.7, 05 §5.9): files and folders only; the
    # one OS call it needs (the atomic rename) comes from b2c-process.
    "b2c-store": {"b2c-ir", "b2c-model", "b2c-process"},
    "b2c-toolchain": {"b2c-ir", "b2c-model", "b2c-process"},
    # Build and run sessions: the cache root and atomic writes come from
    # b2c-store, and the sessions report through b2c-ipc's event types.
    "b2c-build": {
        "b2c-ir", "b2c-model", "b2c-catalog", "b2c-lang", "b2c-codegen",
        "b2c-toolchain", "b2c-process", "b2c-store", "b2c-ipc",
    },
    "b2c-cli": {"b2c-ir", "b2c-model", "b2c-build", "b2c-toolchain"},
    # The IPC contract: types, IDs and decoding only (no Tauri, no services).
    "b2c-ipc": {"b2c-ir", "b2c-model"},
    # The Tauri-free backend services behind every IPC command.
    "b2c-app": {"b2c-ir", "b2c-model", "b2c-build", "b2c-toolchain", "b2c-store", "b2c-ipc"},
    # The desktop shell holds no business logic: it adapts Tauri's IPC to the
    # backend services (b2c-app) and the contract (b2c-ipc), and may use the
    # shared types of the layers below them.
    "blocks2cpp-desktop": {
        "b2c-ir", "b2c-model", "b2c-build", "b2c-toolchain", "b2c-ipc", "b2c-store", "b2c-app",
    },
}

# Test-only exceptions to the rules above, each with its reason.
ALLOWED_DEV_DEPS = {
    # The analyser's end-to-end tests compile the C++ generated from what it
    # accepts; production code of the two crates stays independent.
    ("b2c-lang", "b2c-codegen"),
    # The analyser's scope and robustness tests analyse documents exactly as
    # the resolve stage completes them, including documents it rejects (the
    # editor's preview analyses those too).
    ("b2c-lang", "b2c-catalog"),
    # The backend's generation-parity test compares the editor preview's files
    # with the build's generated files.
    ("b2c-app", "b2c-core-wasm"),
}

# The pure compiler crates and the external crates they may use. Anything
# that does I/O (tempfile, rustix, windows-sys, an async runtime, ...) must
# stay out; add a crate here only after checking that it is pure.
PURE_CRATES = {
    "b2c-ir": {"serde", "thiserror"},
    "b2c-model": {"serde", "serde_json", "sha2", "thiserror"},
    "b2c-catalog": {"serde", "serde_json", "thiserror", "toml"},
    "b2c-lang": {"serde", "serde_json", "thiserror"},
    "b2c-codegen": {"serde", "thiserror"},
    "b2c-core-wasm": {"serde", "serde_json", "thiserror", "wasm-bindgen"},
}


def main() -> int:
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--locked", "--offline", "--no-deps", "--format-version", "1"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    packages = {package["name"]: package for package in metadata["packages"]}
    problems = []

    unknown = sorted(set(packages) - set(ALLOWED_WORKSPACE_DEPS))
    for name in unknown:
        problems.append(f"{name}: a new workspace crate; add its layering rules to tools/check-layering.py")

    for name, package in sorted(packages.items()):
        allowed = ALLOWED_WORKSPACE_DEPS.get(name, set())
        for dependency in package["dependencies"]:
            target = dependency["name"]
            kind = dependency.get("kind") or "normal"
            if target in packages:
                if target in allowed:
                    continue
                if kind == "dev" and (name, target) in ALLOWED_DEV_DEPS:
                    continue
                problems.append(f"{name} may not depend on {target} ({kind} dependency)")
            elif name in PURE_CRATES and kind != "dev" and target not in PURE_CRATES[name]:
                problems.append(
                    f"{name} is a pure compiler crate (no I/O) and may not use the external crate "
                    f"{target}; if it is pure, add it to PURE_CRATES with care"
                )

    if problems:
        print("The crate layering rules (docs/spec/02-architecture.md §2.3) are broken:")
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print(f"Crate layering rules hold for {len(packages)} crates.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
