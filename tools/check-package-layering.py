#!/usr/bin/env python3
"""Checks the npm package layering rules and the package.json supply-chain rules.

Reads the package.json of every pnpm workspace package (the root and the
patterns in pnpm-workspace.yaml) and fails when:

* a package depends on a workspace package it may not use
  (docs/spec/02-architecture.md §2.3):
    - @blocks2cpp/ipc-types has no dependencies;
    - @blocks2cpp/b2c-core-wasm may depend only on ipc-types;
    - @blocks2cpp/blockly-ext may depend only on blockly, ipc-types and
      b2c-core-wasm;
    - @blocks2cpp/catalog-gen is a build-time tool with dev dependencies only,
      and nothing depends on it;
    - @blocks2cpp/desktop may depend on the three runtime packages, not on
      catalog-gen;
* a library package uses an external package at run time that is not on its
  allowlist;
* a dependency is not pinned to an exact version, or a workspace package is
  not referenced with the `workspace:` protocol (otherwise pnpm could fetch a
  same-named package from the registry), or an @blocks2cpp/ name is not a
  workspace package (docs/spec/08-security.md §8.9);
* a package is not private, defines an install lifecycle script (pnpm runs
  those on every install), or is new and has no rules here yet.

Run from the repository root: python3 tools/check-package-layering.py
Check the checker itself:      python3 tools/check-package-layering.py --self-test
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The scope of our own packages. Every name in it must be a workspace package.
SCOPE = "@blocks2cpp/"

IPC_TYPES = "@blocks2cpp/ipc-types"
CORE_WASM = "@blocks2cpp/b2c-core-wasm"
BLOCKLY_EXT = "@blocks2cpp/blockly-ext"
CATALOG_GEN = "@blocks2cpp/catalog-gen"
DESKTOP = "@blocks2cpp/desktop"
SITE = "@blocks2cpp/site"
REPOSITORY_ROOT = "blocks2cpp"

# Workspace packages each package may depend on, in any dependency section.
ALLOWED_WORKSPACE_DEPS: dict[str, set[str]] = {
    IPC_TYPES: set(),
    CORE_WASM: {IPC_TYPES},
    BLOCKLY_EXT: {IPC_TYPES, CORE_WASM},
    CATALOG_GEN: set(),
    DESKTOP: {IPC_TYPES, CORE_WASM, BLOCKLY_EXT},
    # The specification website and the repository root (tooling) use none.
    SITE: set(),
    REPOSITORY_ROOT: set(),
}

# External packages a package may use at run time (dependencies,
# peerDependencies, optionalDependencies). None means no allowlist: the
# package's dependencies are reviewed through the dependency tables instead.
# Dev dependencies (tooling) are never limited here, only pinned.
ALLOWED_RUNTIME_EXTERNALS: dict[str, set[str] | None] = {
    IPC_TYPES: set(),
    CORE_WASM: set(),
    BLOCKLY_EXT: {"blockly"},
    CATALOG_GEN: set(),
    DESKTOP: None,
    SITE: None,
    REPOSITORY_ROOT: None,
}

# Build-time tools: dev dependencies only, and never a dependency of anything.
BUILD_TIME_ONLY = {CATALOG_GEN}

RUNTIME_SECTIONS = ("dependencies", "peerDependencies", "optionalDependencies")
DEV_SECTIONS = ("devDependencies",)

# Scripts that pnpm runs by itself on install. Build steps belong in scripts
# that are run on purpose.
INSTALL_SCRIPTS = ("preinstall", "install", "postinstall", "prepare")

# An exact version: no ranges, tags, URLs, git or file references.
EXACT_VERSION = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$"
)


@dataclass(frozen=True)
class Package:
    """One workspace package: where its package.json is and what it says."""

    path: Path
    manifest: dict

    @property
    def name(self) -> str:
        name = self.manifest.get("name")
        return name if isinstance(name, str) else f"<unnamed package in {self.path}>"


class WorkspaceError(Exception):
    """The workspace cannot be read (a missing or malformed file)."""


def workspace_patterns(workspace_file: Path) -> list[str]:
    """The `packages:` entries of pnpm-workspace.yaml (a plain list of strings)."""
    try:
        lines = workspace_file.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise WorkspaceError(f"cannot read {workspace_file}: {error}") from error
    patterns: list[str] = []
    in_packages = False
    for line in lines:
        text = line.split(" #", 1)[0].rstrip()
        if not text.strip() or text.lstrip().startswith("#"):
            continue
        if not text[0].isspace():
            if text.startswith("packages:") and text != "packages:":
                raise WorkspaceError(
                    f"{workspace_file}: write the packages as a block list (one '- path' per line)"
                )
            in_packages = text == "packages:"
            continue
        if in_packages:
            item = re.fullmatch(r"\s+-\s+(['\"]?)(.+?)\1", text)
            if item is None:
                raise WorkspaceError(f"{workspace_file}: cannot read the packages entry {line!r}")
            patterns.append(item.group(2))
    return patterns


def load_manifest(path: Path) -> dict:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise WorkspaceError(f"cannot read {path}: {error}") from error
    if not isinstance(manifest, dict):
        raise WorkspaceError(f"{path} is not a JSON object")
    return manifest


def workspace_packages(root: Path) -> list[Package]:
    """The root package and every package matched by pnpm-workspace.yaml."""
    include: set[Path] = set()
    exclude: set[Path] = set()
    for pattern in workspace_patterns(root / "pnpm-workspace.yaml"):
        negated = pattern.startswith("!")
        target = exclude if negated else include
        for directory in root.glob(pattern.lstrip("!")):
            if (directory / "package.json").is_file() and "node_modules" not in directory.parts:
                target.add(directory)
    directories = [root] + sorted(include - exclude - {root})
    return [
        Package(directory.relative_to(root), load_manifest(directory / "package.json"))
        for directory in directories
    ]


def dependency_sections(manifest: dict):
    """Yields (section, dependency name, version spec) for every declared dependency."""
    for section in RUNTIME_SECTIONS + DEV_SECTIONS:
        entries = manifest.get(section) or {}
        if not isinstance(entries, dict):
            yield section, None, entries
            continue
        for dependency, spec in sorted(entries.items()):
            yield section, dependency, spec


def check_package(package: Package, workspace_names: set[str]) -> list[str]:
    name = package.name
    manifest = package.manifest
    problems: list[str] = []

    if name not in ALLOWED_WORKSPACE_DEPS:
        return [
            f"{name} ({package.path}): a new workspace package; add its layering rules to "
            "tools/check-package-layering.py"
        ]
    if manifest.get("private") is not True:
        problems.append(f'{name}: must be private ("private": true); nothing here is published')

    scripts = manifest.get("scripts") or {}
    for script in INSTALL_SCRIPTS:
        if isinstance(scripts, dict) and script in scripts:
            problems.append(
                f'{name}: the "{script}" script would run on every pnpm install; '
                "make it a script that is run on purpose"
            )

    allowed_workspace = ALLOWED_WORKSPACE_DEPS[name]
    allowed_externals = ALLOWED_RUNTIME_EXTERNALS[name]
    for section, dependency, spec in dependency_sections(manifest):
        if dependency is None:
            problems.append(f"{name}: {section} must be an object")
            continue
        spec_text = spec if isinstance(spec, str) else json.dumps(spec)
        runtime = section in RUNTIME_SECTIONS

        if name in BUILD_TIME_ONLY and runtime:
            problems.append(
                f"{name} is a build-time tool and may have dev dependencies only, "
                f"not {dependency} in {section}"
            )

        if dependency in workspace_names or dependency.startswith(SCOPE):
            if dependency not in workspace_names:
                problems.append(
                    f"{name} depends on {dependency} ({section}), which is not a workspace "
                    f"package; the {SCOPE} scope is ours, and a registry package with that name "
                    "would be someone else's"
                )
                continue
            if not spec_text.startswith("workspace:"):
                problems.append(
                    f'{name} must refer to {dependency} as "workspace:*" ({section} has '
                    f"{spec_text!r}), so that pnpm never fetches it from the registry"
                )
            if dependency in BUILD_TIME_ONLY:
                problems.append(
                    f"{name} may not depend on {dependency} ({section}): it is a build-time "
                    "tool and never a dependency"
                )
            elif dependency not in allowed_workspace:
                problems.append(f"{name} may not depend on {dependency} ({section} dependency)")
            continue

        if not EXACT_VERSION.match(spec_text):
            problems.append(
                f"{name}: {dependency} must be pinned to an exact version ({section} has "
                f"{spec_text!r})"
            )
        if runtime and allowed_externals is not None and dependency not in allowed_externals:
            allowed = ", ".join(sorted(allowed_externals)) or "nothing"
            problems.append(
                f"{name} may use only {allowed} at run time, not {dependency} ({section})"
            )
    return problems


def check_workspace(root: Path) -> tuple[list[str], int]:
    """All problems in the workspace at `root`, and the number of packages checked."""
    packages = workspace_packages(root)
    problems: list[str] = []
    names: dict[str, Path] = {}
    for package in packages:
        if package.name in names:
            problems.append(
                f"{package.name} is defined twice: in {names[package.name]} and {package.path}"
            )
        names.setdefault(package.name, package.path)
    for package in packages:
        problems.extend(check_package(package, set(names)))
    return problems, len(packages)


# --- Self-test --------------------------------------------------------------

# A workspace that follows every rule, in the repository's layout.
GOOD_WORKSPACE = {
    ".": {"name": REPOSITORY_ROOT, "private": True, "scripts": {"desktop:check": "pnpm -r lint"}},
    "packages/ipc-types": {
        "name": IPC_TYPES,
        "private": True,
        "devDependencies": {"typescript": "5.9.3", "vitest": "4.1.11"},
    },
    "packages/b2c-core-wasm": {
        "name": CORE_WASM,
        "private": True,
        "dependencies": {IPC_TYPES: "workspace:*"},
        "scripts": {"build": "node scripts/build.mjs"},
    },
    "packages/blockly-ext": {
        "name": BLOCKLY_EXT,
        "private": True,
        "dependencies": {"blockly": "12.5.1", IPC_TYPES: "workspace:*", CORE_WASM: "workspace:*"},
        "devDependencies": {"vitest": "4.1.11"},
    },
    "packages/catalog-gen": {
        "name": CATALOG_GEN,
        "private": True,
        "devDependencies": {"typescript": "5.9.3"},
    },
    "apps/desktop": {
        "name": DESKTOP,
        "private": True,
        "dependencies": {
            "react": "19.3.0",
            IPC_TYPES: "workspace:*",
            CORE_WASM: "workspace:*",
            BLOCKLY_EXT: "workspace:*",
        },
        # A prerelease is an exact version too.
        "devDependencies": {"vite": "8.3.1", "typescript": "5.0.0-beta"},
    },
    "site": {"name": SITE, "private": True, "dependencies": {"marked": "18.0.14"}},
}

GOOD_WORKSPACE_FILE = "packages:\n  - apps/desktop\n  - 'packages/*' # all of them\n  - site\n"


def _set(path: str, *keys_and_value):
    """A change to the good workspace: sets manifest[k1][k2]... = value in one package."""

    def apply(workspace: dict) -> None:
        *keys, last, value = keys_and_value
        target = workspace[path]
        for key in keys:
            target = target.setdefault(key, {})
        target[last] = value

    return apply


def _add_package(path: str, manifest: dict):
    def apply(workspace: dict) -> None:
        workspace[path] = manifest

    return apply


# Each case breaks one rule; the checker must report it with this text.
SELF_TEST_CASES = [
    (
        "forbidden edge: blockly-ext uses catalog-gen",
        _set("packages/blockly-ext", "dependencies", CATALOG_GEN, "workspace:*"),
        f"{BLOCKLY_EXT} may not depend on {CATALOG_GEN}",
    ),
    (
        "forbidden edge: the app uses catalog-gen at build time",
        _set("apps/desktop", "devDependencies", CATALOG_GEN, "workspace:*"),
        f"{DESKTOP} may not depend on {CATALOG_GEN}",
    ),
    (
        "upward edge: b2c-core-wasm uses blockly-ext",
        _set("packages/b2c-core-wasm", "dependencies", BLOCKLY_EXT, "workspace:*"),
        f"{CORE_WASM} may not depend on {BLOCKLY_EXT}",
    ),
    (
        "ipc-types gains a dependency",
        _set("packages/ipc-types", "dependencies", "zod", "3.0.0"),
        f"{IPC_TYPES} may use only nothing at run time, not zod",
    ),
    (
        "blockly-ext gains a runtime library",
        _set("packages/blockly-ext", "peerDependencies", "lodash", "4.17.21"),
        f"{BLOCKLY_EXT} may use only blockly at run time, not lodash",
    ),
    (
        "catalog-gen gains a runtime dependency",
        _set("packages/catalog-gen", "dependencies", "smol-toml", "1.0.0"),
        f"{CATALOG_GEN} is a build-time tool and may have dev dependencies only",
    ),
    (
        "a version range",
        _set("packages/blockly-ext", "dependencies", "blockly", "^12.5.1"),
        "blockly must be pinned to an exact version",
    ),
    (
        "a git dependency",
        _set("apps/desktop", "dependencies", "left-pad", "github:someone/left-pad"),
        "left-pad must be pinned to an exact version",
    ),
    (
        "a workspace package referenced by registry version",
        _set("apps/desktop", "dependencies", IPC_TYPES, "0.1.0"),
        f'{DESKTOP} must refer to {IPC_TYPES} as "workspace:*"',
    ),
    (
        "an @blocks2cpp name that is not in the workspace",
        _set("apps/desktop", "dependencies", "@blocks2cpp/telemetry", "1.0.0"),
        "@blocks2cpp/telemetry (dependencies), which is not a workspace package",
    ),
    (
        "a new package without rules",
        _add_package("packages/new-thing", {"name": "@blocks2cpp/new-thing", "private": True}),
        "@blocks2cpp/new-thing (packages/new-thing): a new workspace package",
    ),
    (
        "a package that could be published",
        _set("packages/ipc-types", "private", False),
        f"{IPC_TYPES}: must be private",
    ),
    (
        "an install script",
        _set("packages/b2c-core-wasm", "scripts", "postinstall", "node scripts/build.mjs"),
        f'{CORE_WASM}: the "postinstall" script would run on every pnpm install',
    ),
    (
        "the root uses a workspace package",
        _set(".", "devDependencies", CATALOG_GEN, "workspace:*"),
        f"{REPOSITORY_ROOT} may not depend on {CATALOG_GEN}",
    ),
]


def _write_workspace(
    root: Path, workspace: dict, workspace_file: str = GOOD_WORKSPACE_FILE
) -> None:
    root.mkdir(parents=True)
    (root / "pnpm-workspace.yaml").write_text(workspace_file, encoding="utf-8")
    for path, manifest in workspace.items():
        directory = root / path
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "package.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")


def self_test() -> int:
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="b2c-package-layering-") as scratch:
        good_root = Path(scratch) / "good"
        _write_workspace(good_root, GOOD_WORKSPACE)
        problems, count = check_workspace(good_root)
        if problems or count != len(GOOD_WORKSPACE):
            failures.append(
                f"the good workspace ({count} packages) should pass, but got: {problems}"
            )
        for index, (title, change, expected) in enumerate(SELF_TEST_CASES):
            workspace = json.loads(json.dumps(GOOD_WORKSPACE))
            change(workspace)
            case_root = Path(scratch) / f"case-{index}"
            _write_workspace(case_root, workspace)
            problems, _ = check_workspace(case_root)
            if not any(expected in problem for problem in problems):
                failures.append(
                    f"{title}: expected a problem containing {expected!r}, got {problems}"
                )

        # A workspace file this reader does not understand is an error, not an empty workspace.
        unreadable_root = Path(scratch) / "inline-list"
        _write_workspace(unreadable_root, GOOD_WORKSPACE, "packages: [apps/desktop, site]\n")
        try:
            check_workspace(unreadable_root)
            failures.append("an inline packages list should be refused, not read as no packages")
        except WorkspaceError:
            pass
    if failures:
        print("The package layering self-test failed:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print(f"Package layering self-test passed ({len(SELF_TEST_CASES)} broken workspaces caught).")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="check the checker against generated workspaces that break each rule",
    )
    parser.add_argument(
        "--root", type=Path, default=ROOT, help="the repository root (default: this checkout)"
    )
    arguments = parser.parse_args(argv)
    if arguments.self_test:
        return self_test()

    try:
        problems, count = check_workspace(arguments.root.resolve())
    except WorkspaceError as error:
        print(f"error: {error}")
        return 2
    if problems:
        print(
            "The package rules (docs/spec/02-architecture.md §2.3, "
            "docs/spec/08-security.md §8.9) are broken:"
        )
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print(f"Package layering rules hold for {count} packages.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
