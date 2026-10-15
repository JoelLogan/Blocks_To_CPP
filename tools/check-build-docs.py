#!/usr/bin/env python3
"""Checks that BUILDING.md's pinned versions and apt packages match their sources.

docs/spec/09-quality-and-delivery.md §9.4: build steps are written once, in
BUILDING.md, and pinned tool versions appear only there and in their source of
truth. This check fails when:

* a version in BUILDING.md differs from its source: the "What you need" table
  and every other statement of the version in BUILDING.md (commands such as
  `cargo install wasm-bindgen-cli --version ...` included):
    - wasm-bindgen: the `wasm-bindgen` package in Cargo.lock;
    - Rust: `channel` in rust-toolchain.toml;
    - Node.js: the major version in .node-version (what CI installs) and the
      minimum in package.json's `engines.node` (">=X.Y");
    - pnpm: package.json's `packageManager`;
    - tauri-driver: `TAURI_DRIVER_VERSION` in the workflows;
    - binaryen: `BINARYEN_VERSION` in the workflows, which pin a release;
      BUILDING.md may give a minimum ("N or later") no newer than it;
* the sources disagree: a workflow's `WASM_BINDGEN_VERSION` is not Cargo.lock's
  version, the workflows pin different tauri-driver or binaryen versions, or
  engines.node's major is not .node-version's;
* the Debian and Ubuntu `apt install` command in BUILDING.md does not install
  the packages that the `app` job of .github/workflows/desktop.yml installs
  (apart from APT_ONLY_IN_BUILDING and APT_ONLY_IN_CI), or another workflow
  installs the webview libraries without all of them;
* any other Markdown file the repository has states one of these versions (a
  tool's name followed by its version in the same sentence, as STATEMENTS
  matches it, or the exact wasm-bindgen, Rust or pnpm pin anywhere), or an
  apt install of BUILDING.md's specific packages. Exceptions: dated records
  (RECORDS), dependency-table rows whose version is checked against its source
  instead (DEPENDENCY_ROWS), and the statements listed in ALLOWED.

Run from the repository root: python3 tools/check-build-docs.py
Check the checker itself:      python3 tools/check-build-docs.py --self-test
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

BUILDING = "BUILDING.md"
WORKFLOWS = ".github/workflows"
CI_APT_SOURCE = f"{WORKFLOWS}/desktop.yml"
CI_APT_JOB = "app"

# Packages BUILDING.md installs that CI gets otherwise: the runners have Git, curl
# and the CA certificates, and CI downloads a pinned binaryen release.
APT_ONLY_IN_BUILDING = {"git", "curl", "ca-certificates", "binaryen"}
# Packages CI installs for its own checks: the virtual display of the start check.
APT_ONLY_IN_CI = {"xvfb"}
# Generic packages other documents may name in their own install commands (for
# g++ or Git, for example); every other package of BUILDING.md's list may not.
APT_GENERIC = {"build-essential", "git", "curl", "ca-certificates"}

# Dated records keep the versions they were written with: they are history, not
# instructions (prefixes of repository-relative paths).
RECORDS = ("CHANGELOG.md", "docs/plans/", "docs/adr/")

# Paths never read: installed packages, local tool state (other worktrees among
# them) and fuzz data, as in the anchor check (site/build.mjs).
UNCHECKED = re.compile(r"(^|/)node_modules/|^\.claude/|^fuzz/(corpus|artifacts)/")

# Statements about a tool's version that are not a requirement, by file, with why.
ALLOWED = {
    "packages/catalog-gen/README.md": [
        # Which Node.js releases need the type-stripping flag: a fact about Node.js.
        "Node.js 22.13 to 22.17",
    ],
    "docs/spec/08-security.md": [
        # The Rust release that fixed CVE-2024-24576, not the pinned toolchain.
        "the Rust fix (1.77.2)",
    ],
}

# Dependency tables (docs/spec/09 §9.4: version, release date, licence, why)
# list a pinned tool as a dependency. Such a row, `| `name` ... | version ...|`,
# is checked against the tool's source instead of being reported as a repeat.
DEPENDENCY_ROWS = {"wasm-bindgen": "wasm-bindgen", "tauri-driver": "tauri-driver"}

# What each tool is called in messages.
LABELS = {
    "wasm-bindgen": "wasm-bindgen",
    "rust": "Rust",
    "node": "Node.js",
    "pnpm": "pnpm",
    "tauri-driver": "tauri-driver",
    "binaryen": "binaryen",
}

# The rows of BUILDING.md's "What you need" table, by the start of their first cell.
TABLE_ROWS = {
    "rust": re.compile(r"^Rust\b"),
    "node": re.compile(r"^Node\.js\b"),
    "pnpm": re.compile(r"^pnpm\b"),
    "wasm-bindgen": re.compile(r"^wasm-bindgen\b"),
    "binaryen": re.compile(r"^binaryen\b"),
    "tauri-driver": re.compile(r"^tauri-driver\b"),
}

# Between a tool's name and its version: up to N characters of the same sentence
# and paragraph (no full stop except between digits, no table cell border, no
# blank line). Markdown wraps lines, so a single line break may come between.
_GAP = r"(?:[^.|\n]|(?<=\d)\.(?=\d)|\n(?![ \t]*\n)){0,%d}?"
_END = r"(?![\w-]|\.\d)"


def _after(name: str, version: str, gap: int = 40) -> re.Pattern[str]:
    # A version right after a hyphen belongs to another name (`Apache-2.0`).
    return re.compile(name + _GAP % gap + r"(?<![\w.-])v?(" + version + ")" + _END)


_XYZ = r"\d+\.\d+\.\d+"
_XY = r"\d+\.\d+(?:\.\d+)?"

# Statements of a version: (tool, kind, pattern with the version as group 1). In
# BUILDING.md each must match the source; in any other document each is a repeat.
STATEMENTS: list[tuple[str, str, re.Pattern[str]]] = [
    ("wasm-bindgen", "version", _after(r"wasm-bindgen(?:-cli)?\b", _XYZ)),
    ("tauri-driver", "version", _after(r"tauri-driver\b", _XYZ)),
    ("rust", "version", _after(r"\b(?:Rust|rustc)\b(?![\w-])", r"1\.\d+(?:\.\d+)?")),
    # Node.js in any letter case (`node >=22.14`, `engines.node`), but not as part of a
    # package's name or path (`@types/node`, `node_modules`, `node-v22`).
    ("node", "minimum", _after(r"(?<![/@\w-])(?i:node(?:\.js)?)\b(?![\w-])", _XY)),
    ("pnpm", "version", _after(r"\bpnpm\b(?![\w-])", _XY)),
    # binaryen's release: after the word version or release (`binaryen release 133`), or
    # right after the name (`binaryen 133`, "`wasm-opt` is 133"); a number further away is
    # something else (`binaryen (step 1 on Linux)`).
    (
        "binaryen",
        "version",
        re.compile(
            r"\b(?:binaryen|wasm-opt)\b(?:"
            + _GAP % 60
            + r"\b(?:version|release)[ _]?|`?(?:'s)?[ \t]+(?:is[ \t]+)?`?)(\d+)"
            + _END
        ),
    ),
]

# Node.js majors, checked in BUILDING.md only (other documents may name a major).
NODE_MAJORS = [
    re.compile(r"\bNode(?:\.js)?\s+(\d+)\b(?!\.\d)"),
    re.compile(r"\blatest-v(\d+)\.x\b"),
    re.compile(r"\bnode-v(\d+)\b"),
    re.compile(r"\bNodeJS\.(\d+)\b"),
]

_APT = re.compile(r"\bapt(?:-get)?\s+install\b")
_PACKAGE = re.compile(r"^[a-z0-9][a-z0-9.+-]*$")


@dataclass
class Sources:
    """The pinned versions as their sources give them (None when unreadable)."""

    wasm_bindgen: str | None = None
    rust: str | None = None
    node_major: str | None = None
    node_minimum: str | None = None
    pnpm: str | None = None
    tauri_driver: str | None = None
    binaryen: str | None = None
    apt: set[str] | None = None
    # Where each came from, for messages.
    origin: dict[str, str] = field(default_factory=dict)


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def read(root: Path, relative: str, problems: list[str]) -> str | None:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        problems.append(f"{relative}: cannot be read ({error.strerror or error})")
        return None


def apt_commands(text: str) -> list[tuple[int, list[str]]]:
    """Each `apt install` / `apt-get install` command: its line and its packages.

    Continuation lines (ending in a backslash) are joined; options, comments and
    whatever follows the command on its line (inline code ends, `&&`, `;`) are
    left out.
    """
    commands = []
    for match in _APT.finditer(text):
        end = match.end()
        command = ""
        while True:
            newline = text.find("\n", end)
            line = text[end:] if newline == -1 else text[end:newline]
            stripped = line.rstrip()
            command += " " + stripped.removesuffix("\\")
            if not stripped.endswith("\\") or newline == -1:
                break
            end = newline + 1
        command = re.split(r"#|`|&&|;|\||\)", command, maxsplit=1)[0]
        packages = [
            word for word in command.split() if not word.startswith("-") and _PACKAGE.match(word)
        ]
        commands.append((line_of(text, match.start()), packages))
    return commands


def job_block(workflow: str, job: str) -> str | None:
    """The text of `jobs.<job>` in a workflow (two-space indented job keys)."""
    match = re.search(rf"^  {re.escape(job)}:[ \t]*$", workflow, re.M)
    if match is None:
        return None
    following = re.search(r"^  [A-Za-z0-9_-]+:[ \t]*$|^\S", workflow[match.end() :], re.M)
    end = match.end() + (following.start() if following else len(workflow) - match.end())
    return workflow[match.start() : end]


def workflow_env(root: Path, name: str) -> list[tuple[str, int, str]]:
    """Every `NAME: value` in the workflows: (file, line, value)."""
    found = []
    pattern = re.compile(rf"^[ \t]*{re.escape(name)}:[ \t]*[\"']?([^\"'\s#]+)[\"']?", re.M)
    for path in sorted((root / WORKFLOWS).glob("*.yml")):
        text = path.read_text(encoding="utf-8")
        relative = path.relative_to(root).as_posix()
        for match in pattern.finditer(text):
            found.append((relative, line_of(text, match.start()), match.group(1)))
    return found


def one_pin(found: list[tuple[str, int, str]], name: str, problems: list[str]) -> str | None:
    """The value every workflow gives `name`, or None (with a problem if they differ)."""
    values = sorted({value for _, _, value in found})
    if len(values) > 1:
        places = ", ".join(f"{path}:{line} {value}" for path, line, value in found)
        problems.append(f"the workflows pin different {name} values: {places}")
        return None
    return values[0] if values else None


def read_sources(root: Path, problems: list[str]) -> Sources:
    sources = Sources()

    lock = read(root, "Cargo.lock", problems)
    if lock is not None:
        try:
            packages = tomllib.loads(lock).get("package", [])
        except tomllib.TOMLDecodeError as error:
            problems.append(f"Cargo.lock: not TOML ({error})")
            packages = []
        versions = sorted({p.get("version") for p in packages if p.get("name") == "wasm-bindgen"})
        if len(versions) == 1:
            sources.wasm_bindgen = versions[0]
            sources.origin["wasm-bindgen"] = "Cargo.lock"
        else:
            problems.append(f"Cargo.lock: expected one wasm-bindgen version, found {versions}")

    toolchain = read(root, "rust-toolchain.toml", problems)
    if toolchain is not None:
        try:
            channel = str(tomllib.loads(toolchain).get("toolchain", {}).get("channel", ""))
        except tomllib.TOMLDecodeError as error:
            problems.append(f"rust-toolchain.toml: not TOML ({error})")
            channel = ""
        if re.fullmatch(_XYZ, channel):
            sources.rust = channel
            sources.origin["rust"] = "rust-toolchain.toml"
        else:
            problems.append(f"rust-toolchain.toml: the channel {channel!r} is not a pinned version")

    node_version = read(root, ".node-version", problems)
    if node_version is not None:
        major = re.match(r"v?(\d+)", node_version.strip())
        if major:
            sources.node_major = major.group(1)
            sources.origin["node-major"] = ".node-version"
        else:
            problems.append(f".node-version: no Node.js version in {node_version.strip()!r}")

    manifest_text = read(root, "package.json", problems)
    try:
        manifest = json.loads(manifest_text) if manifest_text is not None else None
    except json.JSONDecodeError as error:
        problems.append(f"package.json: not JSON ({error})")
        manifest = None
    if isinstance(manifest, dict):
        engines = str(manifest.get("engines", {}).get("node", ""))
        minimum = re.fullmatch(r">=\s*(\d+\.\d+(?:\.\d+)?)", engines.strip())
        if minimum:
            sources.node_minimum = minimum.group(1)
            sources.origin["node"] = "package.json engines.node"
            if sources.node_major and minimum.group(1).split(".")[0] != sources.node_major:
                problems.append(
                    f"package.json: engines.node {engines} is not Node.js "
                    f"{sources.node_major} (.node-version)"
                )
        else:
            problems.append(f'package.json: engines.node should be ">=X.Y", not {engines!r}')
        manager = re.fullmatch(r"pnpm@(\d+\.\d+\.\d+)(?:\+.*)?", manifest.get("packageManager", ""))
        if manager:
            sources.pnpm = manager.group(1)
            sources.origin["pnpm"] = "package.json packageManager"
        else:
            problems.append('package.json: packageManager should be "pnpm@X.Y.Z"')

    for path, line, value in workflow_env(root, "WASM_BINDGEN_VERSION"):
        if sources.wasm_bindgen and value != sources.wasm_bindgen:
            problems.append(
                f"{path}:{line}: WASM_BINDGEN_VERSION {value}, but Cargo.lock has "
                f"wasm-bindgen {sources.wasm_bindgen}"
            )
    sources.tauri_driver = one_pin(
        workflow_env(root, "TAURI_DRIVER_VERSION"), "TAURI_DRIVER_VERSION", problems
    )
    sources.origin["tauri-driver"] = "TAURI_DRIVER_VERSION in the workflows"
    sources.binaryen = one_pin(workflow_env(root, "BINARYEN_VERSION"), "BINARYEN_VERSION", problems)
    sources.origin["binaryen"] = "BINARYEN_VERSION in the workflows"

    workflow = read(root, CI_APT_SOURCE, problems)
    if workflow is not None:
        job = job_block(workflow, CI_APT_JOB)
        commands = apt_commands(job) if job else []
        if len(commands) != 1:
            problems.append(
                f"{CI_APT_SOURCE}: expected one apt-get install in the {CI_APT_JOB} job, "
                f"found {len(commands)}"
            )
        else:
            sources.apt = set(commands[0][1])
            sources.origin["apt"] = f"the {CI_APT_JOB} job of {CI_APT_SOURCE}"
    return sources


def expected(sources: Sources, tool: str, kind: str) -> str | None:
    return {
        ("wasm-bindgen", "version"): sources.wasm_bindgen,
        ("tauri-driver", "version"): sources.tauri_driver,
        ("rust", "version"): sources.rust,
        ("node", "minimum"): sources.node_minimum,
        ("pnpm", "version"): sources.pnpm,
        ("binaryen", "version"): sources.binaryen,
    }[(tool, kind)]


def origin_of(sources: Sources, tool: str) -> str:
    return sources.origin.get(tool, "its source")


def check_table(text: str, sources: Sources, problems: list[str]) -> None:
    """The "What you need" table of BUILDING.md."""
    heading = re.search(r"^## What you need[ \t]*$", text, re.M)
    if heading is None:
        problems.append(f'{BUILDING}: no "## What you need" section')
        return
    rows: dict[str, tuple[int, str]] = {}
    first = line_of(text, heading.end())
    in_table = False
    for index, line in enumerate(text[heading.end() :].split("\n")):
        if not line.startswith("|"):
            if in_table:
                break
            continue
        in_table = True
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        label = cells[0].replace("`", "")
        for tool, pattern in TABLE_ROWS.items():
            if pattern.match(label) and len(cells) > 1:
                rows[tool] = (first + index, cells[1])

    wanted = {
        "wasm-bindgen": sources.wasm_bindgen,
        "rust": sources.rust,
        "node": sources.node_minimum,
        "pnpm": sources.pnpm,
        "tauri-driver": sources.tauri_driver,
        "binaryen": sources.binaryen,
    }
    for tool, value in wanted.items():
        if value is None:
            continue
        if tool not in rows:
            problems.append(
                f'{BUILDING}: the "What you need" table has no {LABELS[tool]} row '
                f"({value}, from {origin_of(sources, tool)})"
            )
            continue
        line, cell = rows[tool]
        where = f"{BUILDING}:{line}"
        if tool == "binaryen":
            numbers = re.findall(r"\d+", cell)
            if not numbers:
                problems.append(f"{where}: the binaryen row names no version")
            elif "or later" in cell:
                if int(numbers[0]) > int(value):
                    problems.append(
                        f"{where}: binaryen {numbers[0]} or later, but CI uses release {value} "
                        f"({origin_of(sources, tool)})"
                    )
            elif numbers[0] != value:
                problems.append(
                    f"{where}: binaryen {numbers[0]}, but {origin_of(sources, tool)} is {value}"
                )
        elif tool == "node":
            tokens = re.findall(r"\d+(?:\.\d+)*", cell)
            if value not in tokens:
                problems.append(
                    f"{where}: the Node.js row should name {value} "
                    f"({origin_of(sources, tool)}), not {cell!r}"
                )
            for token in tokens:
                if sources.node_major and token.split(".")[0] != sources.node_major:
                    problems.append(
                        f"{where}: Node.js {token}, but .node-version is {sources.node_major}"
                    )
        else:
            found = re.search(_XYZ, cell)
            if found is None or found.group(0) != value:
                shown = found.group(0) if found else repr(cell)
                problems.append(
                    f"{where}: {LABELS[tool]} {shown}, but {origin_of(sources, tool)} has {value}"
                )


def check_building(root: Path, sources: Sources, problems: list[str]) -> set[str]:
    """BUILDING.md against the sources; returns the packages its Debian apt command installs."""
    text = read(root, BUILDING, problems)
    if text is None:
        return set()
    check_table(text, sources, problems)

    # Every other statement of a version in BUILDING.md.
    for tool, kind, pattern in STATEMENTS:
        value = expected(sources, tool, kind)
        if value is None:
            continue
        for match in pattern.finditer(text):
            if match.group(1) != value:
                problems.append(
                    f"{BUILDING}:{line_of(text, match.start(1))}: {LABELS[tool]} "
                    f"{match.group(1)}, but {origin_of(sources, tool)} has {value}"
                )
    if sources.node_major:
        for pattern in NODE_MAJORS:
            for match in pattern.finditer(text):
                if match.group(1) != sources.node_major:
                    problems.append(
                        f"{BUILDING}:{line_of(text, match.start(1))}: Node.js {match.group(1)}, "
                        f"but .node-version is {sources.node_major}"
                    )

    # The Debian and Ubuntu packages.
    section = re.search(r"^## Linux: Debian and Ubuntu[ \t]*$", text, re.M)
    commands = apt_commands(text[section.end() :]) if section else []
    if not commands:
        problems.append(f'{BUILDING}: no apt install in "## Linux: Debian and Ubuntu"')
        return set()
    line = line_of(text, section.end()) + commands[0][0] - 1
    documented = set(commands[0][1])
    if sources.apt is not None:
        missing = sorted((sources.apt - APT_ONLY_IN_CI) - documented)
        extra = sorted(documented - sources.apt - APT_ONLY_IN_BUILDING)
        if missing:
            problems.append(
                f"{BUILDING}:{line}: the apt packages lack {', '.join(missing)}, which "
                f"{origin_of(sources, 'apt')} installs"
            )
        if extra:
            problems.append(
                f"{BUILDING}:{line}: the apt packages add {', '.join(extra)}, which "
                f"{origin_of(sources, 'apt')} does not install (or list them in "
                "APT_ONLY_IN_BUILDING with a reason)"
            )
        # Every workflow that installs the webview libraries installs all of them.
        needed = sources.apt - APT_ONLY_IN_CI
        for path in sorted((root / WORKFLOWS).glob("*.yml")):
            workflow = path.read_text(encoding="utf-8")
            for command_line, packages in apt_commands(workflow):
                if "libwebkit2gtk-4.1-dev" in packages and not needed <= set(packages):
                    problems.append(
                        f"{path.relative_to(root).as_posix()}:{command_line}: installs the "
                        f"webview libraries without {', '.join(sorted(needed - set(packages)))}"
                    )
    return documented


def markdown_files(root: Path) -> list[str]:
    """The repository's Markdown files (tracked or new, not ignored), relative to `root`.

    Outside a git checkout (the self-test's repositories), every Markdown file below `root`.
    """
    names: list[str] = []
    try:
        if not (root / ".git").exists():
            raise FileNotFoundError(root / ".git")
        listed = subprocess.run(
            ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "*.md"],
            cwd=root,
            check=True,
            capture_output=True,
        ).stdout.decode("utf-8")
        names = [name for name in listed.split("\0") if name]
    except (OSError, subprocess.CalledProcessError):
        names = [path.relative_to(root).as_posix() for path in root.rglob("*.md")]
    return sorted(
        name for name in set(names) if not UNCHECKED.search(name) and (root / name).is_file()
    )


def blank(text: str, start: int, end: int) -> str:
    """`text` with [start, end) replaced by spaces, keeping line breaks and offsets."""
    return text[:start] + re.sub(r"[^\n]", " ", text[start:end]) + text[end:]


def check_others(
    root: Path, sources: Sources, documented: set[str], problems: list[str]
) -> int:
    """Every other Markdown file; returns how many were read.

    `documented` is BUILDING.md's apt package list.
    """
    pins = {
        "wasm-bindgen": sources.wasm_bindgen,
        "rust": sources.rust,
        "pnpm": sources.pnpm,
    }
    exact = [
        (tool, re.compile(r"(?<![\w.])" + re.escape(value) + _END))
        for tool, value in pins.items()
        if value
    ]
    specific = documented - APT_GENERIC
    dependency_row = re.compile(
        r"^\|\s*`(" + "|".join(map(re.escape, DEPENDENCY_ROWS)) + r")`[^|]*\|([^|]*)\|", re.M
    )

    files = [name for name in markdown_files(root) if name != BUILDING]
    for name in files:
        if name.startswith(RECORDS):
            continue
        text = (root / name).read_text(encoding="utf-8")
        for phrase in ALLOWED.get(name, []):
            start = text.find(phrase)
            if start == -1:
                problems.append(
                    f"{name}: no longer says {phrase!r}; remove it from ALLOWED in "
                    "tools/check-build-docs.py"
                )
            while start != -1:
                text = blank(text, start, start + len(phrase))
                start = text.find(phrase)

        for match in dependency_row.finditer(text):
            tool = DEPENDENCY_ROWS[match.group(1)]
            value = sources.wasm_bindgen if tool == "wasm-bindgen" else sources.tauri_driver
            found = re.search(_XYZ, match.group(2))
            if value and (found is None or found.group(0) != value):
                problems.append(
                    f"{name}:{line_of(text, match.start())}: the dependency row of "
                    f"{match.group(1)} says {found.group(0) if found else 'no version'}, but "
                    f"{origin_of(sources, tool)} has {value}"
                )
            text = blank(text, match.start(), match.end())

        reported: set[tuple[int, str]] = set()

        def report(offset: int, tool: str, value: str) -> None:
            line = line_of(text, offset)
            if (line, tool) not in reported:
                reported.add((line, tool))
                problems.append(
                    f"{name}:{line}: states {LABELS[tool]} {value}; pinned versions are written "
                    f"only in {BUILDING} (link to {BUILDING}#what-you-need instead)"
                )

        for tool, _, pattern in STATEMENTS:
            for match in pattern.finditer(text):
                report(match.start(1), tool, match.group(1))
        for tool, pattern in exact:
            for match in pattern.finditer(text):
                report(match.start(), tool, match.group(0))
        for line, packages in apt_commands(text):
            named = sorted(set(packages) & specific)
            if named:
                problems.append(
                    f"{name}:{line}: an apt install of {', '.join(named)} repeats "
                    f"{BUILDING}'s package list (link to {BUILDING}#linux-debian-and-ubuntu "
                    "instead)"
                )
    return len(files)


def _place(problem: str) -> tuple[str, int]:
    """A problem's file and line, for sorting (line 0 when it names none)."""
    match = re.match(r"([^:\s]+):(\d+):", problem)
    return (match.group(1), int(match.group(2))) if match else (problem.split(":")[0], 0)


def check(root: Path) -> tuple[list[str], int]:
    """Every problem of the repository at `root`, by file and line, and how many other
    Markdown files were read."""
    problems: list[str] = []
    sources = read_sources(root, problems)
    documented = check_building(root, sources, problems)
    count = check_others(root, sources, documented, problems)
    return sorted(problems, key=_place), count


# ---------------------------------------------------------------------------
# Self-test: a small repository that passes, and changes to it that each break
# one rule; then a wasm-bindgen bump in a scratch copy of this repository.

GOOD: dict[str, str] = {
    "Cargo.lock": (
        "version = 4\n\n"
        '[[package]]\nname = "core"\nversion = "0.1.0"\ndependencies = [\n "wasm-bindgen",\n]\n\n'
        '[[package]]\nname = "wasm-bindgen"\nversion = "0.2.129"\n'
        'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
    ),
    "rust-toolchain.toml": '[toolchain]\nchannel = "1.97.0"\n',
    ".node-version": "22\n",
    "package.json": json.dumps(
        {"name": "repo", "packageManager": "pnpm@11.27.1", "engines": {"node": ">=22.14"}}
    ),
    ".github/workflows/desktop.yml": """name: Desktop
env:
  WASM_BINDGEN_VERSION: 0.2.129
jobs:
  test-web:
    env:
      BINARYEN_VERSION: "133"
    steps:
      - run: echo
  app:
    steps:
      - name: Install the webview libraries (Linux)
        run: |
          sudo apt-get update
          sudo apt-get install --yes --no-install-recommends \\
            build-essential pkg-config libwebkit2gtk-4.1-dev libsoup-3.0-dev xvfb
  e2e:
    env:
      TAURI_DRIVER_VERSION: 2.1.0
    steps:
      - run: |
          sudo apt-get install --yes build-essential pkg-config \\
            libwebkit2gtk-4.1-dev libsoup-3.0-dev webkit2gtk-driver xvfb
""",
    ".github/workflows/nightly.yml": """name: Nightly
env:
  WASM_BINDGEN_VERSION: 0.2.129
  TAURI_DRIVER_VERSION: 2.1.0
jobs:
  e2e:
    steps:
      - run: echo
""",
    BUILDING: """# Building

## What you need

| Tool                       | Version                                             | Used for |
| -------------------------- | --------------------------------------------------- | -------- |
| Rust, through rustup       | 1.97.0, from `rust-toolchain.toml`                  | the app  |
| Node.js                    | 22, from 22.14 on                                   | the app  |
| pnpm                       | 11.27.1, from `package.json`                        | the app  |
| wasm-bindgen CLI           | 0.2.129, the `wasm-bindgen` version in `Cargo.lock` | the core |
| binaryen's `wasm-opt`      | 108 or later (optional)                             | the core |
| tauri-driver (E2E only)    | 2.1.0, from the workflows                           | E2E      |
| g++ (only to run programs) | 11 or later                                         | programs |

## Linux: Debian and Ubuntu

1. Install the packages:

   ```sh
   sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev \\
     libsoup-3.0-dev binaryen git curl ca-certificates
   ```

2. Install Node.js 22 (if you already have Node.js 22.14 or later, skip this):

   ```sh
   curl -fsSLO https://nodejs.org/dist/latest-v22.x/SHASUMS256.txt
   ```

3. Get the tools:

   ```sh
   cargo install wasm-bindgen-cli --version 0.2.129 --locked
   ```

## Windows

```powershell
winget install --exact --id OpenJS.NodeJS.22
```

## The command-line tool

On Debian, step 1 can be `sudo apt install build-essential git curl`.
""",
    "README.md": "# Repo\n\nBuild it as [BUILDING.md](BUILDING.md) says; it needs pnpm 11.\n",
    "docs/developer-guide/dependencies.md": (
        "# Dependencies\n\n"
        "| Crate | Version (released) | Licence |\n"
        "| ----- | ------------------ | ------- |\n"
        "| `wasm-bindgen` (through `core`) | 0.2.129 (2026-09-25) | MIT |\n"
        "| `tauri-driver` (a tool) | 2.1.0 (2024-01-01) | MIT |\n"
    ),
    "docs/user-guide/getting-started.md": (
        "# Getting started\n\nInstall g++:\n\n```sh\nsudo apt install g++\n```\n"
    ),
    "packages/catalog-gen/README.md": (
        "# catalog-gen\n\nType stripping (`--experimental-strip-types`, needed on\n"
        "Node.js 22.13 to 22.17 and accepted by later versions).\n"
    ),
    "CHANGELOG.md": "# Changelog\n\n- wasm-bindgen 0.2.128, Rust 1.96.0 and pnpm 11.20.0.\n",
    "docs/adr/0009-e2e.md": "# ADR\n\nWe use `tauri-driver` (2.1.0, installed with cargo).\n",
    # A licence's version after a tool's name is not the tool's version.
    "docs/security/reviews.md": (
        "# Reviews\n\n`pnpm licenses list` found MIT (419), Apache-2.0 (21) and\nMPL-2.0 (4).\n"
    ),
    # Node's type package and a binaryen without a number are not versions of them.
    "docs/developer-guide/testing.md": (
        "# Testing\n\n`@types/node` 24.19.0 needs `undici-types@6.21.0`; install\n"
        "binaryen (step 1 on Linux).\n"
    ),
}


def _replace(path: str, old: str, new: str):
    def apply(files: dict[str, str]) -> None:
        assert old in files[path], f"self-test fixture: {old!r} is not in {path}"
        files[path] = files[path].replace(old, new)

    return apply


def _add(path: str, text: str):
    def apply(files: dict[str, str]) -> None:
        files[path] = text

    return apply


# Each case breaks one rule; the check must report every one of its texts.
SELF_TEST_CASES = [
    (
        "a scratch wasm-bindgen bump in Cargo.lock",
        _replace("Cargo.lock", 'version = "0.2.129"', 'version = "0.2.130"'),
        [
            "BUILDING.md:10: wasm-bindgen 0.2.129, but Cargo.lock has 0.2.130",
            "BUILDING.md:33: wasm-bindgen 0.2.129, but Cargo.lock has 0.2.130",
            ".github/workflows/desktop.yml:3: WASM_BINDGEN_VERSION 0.2.129, but Cargo.lock",
            "docs/developer-guide/dependencies.md:5: the dependency row of wasm-bindgen says "
            "0.2.129, but Cargo.lock has 0.2.130",
        ],
    ),
    (
        "a Rust toolchain bump",
        _replace("rust-toolchain.toml", "1.97.0", "1.98.0"),
        ["BUILDING.md:7: Rust 1.97.0, but rust-toolchain.toml has 1.98.0"],
    ),
    (
        "a toolchain channel that is not a version",
        _replace("rust-toolchain.toml", "1.97.0", "stable"),
        ["rust-toolchain.toml: the channel 'stable' is not a pinned version"],
    ),
    (
        "a newer Node.js minimum",
        _replace("package.json", ">=22.14", ">=22.15"),
        [
            "BUILDING.md:8: the Node.js row should name 22.15",
            "BUILDING.md:24: Node.js 22.14, but package.json engines.node has 22.15",
        ],
    ),
    (
        "a new Node.js major in CI",
        _replace(".node-version", "22", "24"),
        [
            "package.json: engines.node >=22.14 is not Node.js 24",
            "BUILDING.md:8: Node.js 22, but .node-version is 24",
            "BUILDING.md:24: Node.js 22, but .node-version is 24",
            "BUILDING.md:27: Node.js 22, but .node-version is 24",
            "BUILDING.md:39: Node.js 22, but .node-version is 24",
        ],
    ),
    (
        "a pnpm bump",
        _replace("package.json", "pnpm@11.27.1", "pnpm@11.28.0"),
        ["BUILDING.md:9: pnpm 11.27.1, but package.json packageManager has 11.28.0"],
    ),
    (
        "a tauri-driver bump in every workflow",
        lambda files: [
            _replace(path, "TAURI_DRIVER_VERSION: 2.1.0", "TAURI_DRIVER_VERSION: 2.2.0")(files)
            for path in (".github/workflows/desktop.yml", ".github/workflows/nightly.yml")
        ],
        [
            "BUILDING.md:12: tauri-driver 2.1.0, but TAURI_DRIVER_VERSION in the workflows has "
            "2.2.0",
            "docs/developer-guide/dependencies.md:6: the dependency row of tauri-driver says 2.1.0",
        ],
    ),
    (
        "workflows that pin different tauri-driver versions",
        _replace(".github/workflows/nightly.yml", "DRIVER_VERSION: 2.1.0", "DRIVER_VERSION: 2.2.0"),
        ["the workflows pin different TAURI_DRIVER_VERSION values"],
    ),
    (
        "a workflow's wasm-bindgen CLI that is not Cargo.lock's",
        _replace(
            ".github/workflows/nightly.yml", "BINDGEN_VERSION: 0.2.129", "BINDGEN_VERSION: 0.2.1"
        ),
        [".github/workflows/nightly.yml:3: WASM_BINDGEN_VERSION 0.2.1, but Cargo.lock has"],
    ),
    (
        "CI's binaryen older than BUILDING.md's minimum",
        _replace(".github/workflows/desktop.yml", '"133"', '"100"'),
        ["BUILDING.md:11: binaryen 108 or later, but CI uses release 100"],
    ),
    (
        "a package CI installs and BUILDING.md lacks",
        _replace(
            ".github/workflows/desktop.yml",
            "libsoup-3.0-dev xvfb",
            "libsoup-3.0-dev librsvg2-dev xvfb",
        ),
        [
            "BUILDING.md:20: the apt packages lack librsvg2-dev",
            ".github/workflows/desktop.yml:22: installs the webview libraries without librsvg2-dev",
        ],
    ),
    (
        "a package BUILDING.md installs and CI does not",
        _replace(BUILDING, "libsoup-3.0-dev binaryen", "libsoup-3.0-dev libfoo-dev binaryen"),
        ["BUILDING.md:20: the apt packages add libfoo-dev"],
    ),
    (
        "a missing row",
        _replace(
            BUILDING, "| tauri-driver (E2E only)    | 2.1.0", "| Other tool                 | 2.1.0"
        ),
        ['BUILDING.md: the "What you need" table has no tauri-driver row (2.1.0'],
    ),
    (
        "BUILDING.md contradicting itself",
        _replace(BUILDING, "Node.js 22.14 or later", "Node.js 22.13 or later"),
        ["BUILDING.md:24: Node.js 22.13, but package.json engines.node has 22.14"],
    ),
    (
        "another document repeating the wasm-bindgen install command",
        _add(
            "docs/guide.md",
            "# Guide\n\n```sh\ncargo install wasm-bindgen-cli --version 0.2.129\n```\n",
        ),
        ["docs/guide.md:4: states wasm-bindgen 0.2.129; pinned versions are written only in"],
    ),
    (
        "another document with an old wasm-bindgen version",
        _add("docs/guide.md", "# Guide\n\nIt needs the wasm-bindgen CLI\n0.2.100 or later.\n"),
        ["docs/guide.md:4: states wasm-bindgen 0.2.100"],
    ),
    (
        "another document with the wasm-bindgen version alone",
        _add("docs/guide.md", "# Guide\n\nThe CLI of the version in `Cargo.lock` (0.2.129).\n"),
        ["docs/guide.md:3: states wasm-bindgen 0.2.129"],
    ),
    (
        "another document with a Node.js minimum",
        _add("site/README.md", "# Site\n\nRequirements: Node.js 22.13 or later, and pnpm.\n"),
        ["site/README.md:3: states Node.js 22.13"],
    ),
    (
        "an allowed statement in another file",
        _add("docs/guide.md", "# Guide\n\nNode.js 22.13 to 22.17 need a flag.\n"),
        ["docs/guide.md:3: states Node.js 22.13"],
    ),
    (
        "an allowed statement that is gone",
        _replace("packages/catalog-gen/README.md", "Node.js 22.13 to 22.17", "Node.js before 23"),
        ["packages/catalog-gen/README.md: no longer says 'Node.js 22.13 to 22.17'"],
    ),
    (
        "another document with the pnpm version",
        _add("CONTRIBUTING.md", "# Contributing\n\nUse pnpm 11.27.1 (Corepack).\n"),
        ["CONTRIBUTING.md:3: states pnpm 11.27.1"],
    ),
    (
        "another document with the tauri-driver install command",
        _add(
            "apps/desktop/e2e/README.md",
            "# E2E\n\n```sh\ncargo install tauri-driver --locked --version 2.1.0\n```\n",
        ),
        ["apps/desktop/e2e/README.md:4: states tauri-driver 2.1.0"],
    ),
    (
        "another document with tauri-driver's version on the next line",
        _add(
            "docs/testing.md",
            "# Testing\n\nThe tests drive `tauri-driver`\n2.1.0 (Apache-2.0 OR MIT).\n",
        ),
        ["docs/testing.md:4: states tauri-driver 2.1.0"],
    ),
    (
        "another document with the Rust version",
        _add("docs/guide.md", "# Guide\n\nThe toolchain is Rust 1.97.0.\n"),
        ["docs/guide.md:3: states Rust 1.97"],
    ),
    (
        "another document with binaryen's release",
        _add("docs/guide.md", "# Guide\n\nCI uses binaryen release 133.\n"),
        ["docs/guide.md:3: states binaryen 133"],
    ),
    (
        "another document with binaryen's version after a version number",
        _add(
            "docs/guide.md",
            "# Guide\n\nThe `binaryen` package of Ubuntu 24.04, version 108, works.\n",
        ),
        ["docs/guide.md:3: states binaryen 108"],
    ),
    (
        "another document with binaryen's release right after its name",
        _add("docs/guide.md", "# Guide\n\nCI pins binaryen 133.\n"),
        ["docs/guide.md:3: states binaryen 133"],
    ),
    (
        "another document with wasm-opt's release after 'is'",
        _add("docs/guide.md", "# Guide\n\nThe pinned `wasm-opt` is 133.\n"),
        ["docs/guide.md:3: states binaryen 133"],
    ),
    (
        "another document with a lower-case node minimum",
        _add("docs/guide.md", "# Guide\n\nIt needs node >=22.14.\n"),
        ["docs/guide.md:3: states Node.js 22.14"],
    ),
    (
        "another document with the root's engines.node",
        _add("docs/guide.md", "# Guide\n\nThe root `engines.node` is `>=22.14`.\n"),
        ["docs/guide.md:3: states Node.js 22.14"],
    ),
    (
        "another document repeating the apt list",
        _add(
            "docs/developer-guide/testing.md",
            "# Testing\n\n```sh\n"
            "sudo apt install build-essential \\\n  libwebkit2gtk-4.1-dev\n```\n",
        ),
        ["docs/developer-guide/testing.md:4: an apt install of libwebkit2gtk-4.1-dev repeats"],
    ),
    (
        "another document installing binaryen with apt",
        _add("packages/core/README.md", "# Core\n\n`sudo apt install binaryen` on Debian.\n"),
        ["packages/core/README.md:3: an apt install of binaryen repeats"],
    ),
    (
        "a dependency row with another version",
        _replace(
            "docs/developer-guide/dependencies.md",
            "| 0.2.129 (2026-09-25)",
            "| 0.2.128 (2026-09-01)",
        ),
        ["docs/developer-guide/dependencies.md:5: the dependency row of wasm-bindgen says 0.2.128"],
    ),
]


def _write(root: Path, files: dict[str, str]) -> None:
    for relative, text in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")


# The files of this repository a scratch copy needs: the sources and BUILDING.md
# (the other Markdown is left out, so the bump is the only change it sees).
SCRATCH_FILES = ["Cargo.lock", "rust-toolchain.toml", ".node-version", "package.json", BUILDING]


def _scratch_bump(scratch: Path, failures: list[str]) -> None:
    """Copies this repository's sources and BUILDING.md, bumps wasm-bindgen, expects failures."""
    copy = scratch / "this-repository"
    for relative in SCRATCH_FILES:
        (copy / relative).parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / relative, copy / relative)
    shutil.copytree(ROOT / WORKFLOWS, copy / WORKFLOWS)
    for relative in ALLOWED:
        (copy / relative).parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / relative, copy / relative)

    before, _ = check(copy)
    if before:
        failures.append(f"a scratch copy of this repository should pass first, but got: {before}")
        return
    lock = (copy / "Cargo.lock").read_text(encoding="utf-8")
    version = re.search(r'\nname = "wasm-bindgen"\nversion = "(\d+)\.(\d+)\.(\d+)"', lock)
    if version is None:
        failures.append("Cargo.lock of this repository has no wasm-bindgen package")
        return
    old = ".".join(version.groups())
    new = f"{version.group(1)}.{version.group(2)}.{int(version.group(3)) + 1}"
    entry = 'name = "wasm-bindgen"\nversion = "{}"'
    (copy / "Cargo.lock").write_text(
        lock.replace(entry.format(old), entry.format(new)), encoding="utf-8"
    )
    after, _ = check(copy)
    if not any(
        problem.startswith(f"{BUILDING}:") and f"Cargo.lock has {new}" in problem
        for problem in after
    ):
        failures.append(
            f"bumping wasm-bindgen {old} to {new} in a scratch copy should fail BUILDING.md, "
            f"but got: {after}"
        )


def self_test() -> int:
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="b2c-build-docs-") as scratch_name:
        scratch = Path(scratch_name)
        good = scratch / "good"
        _write(good, GOOD)
        problems, count = check(good)
        if problems:
            failures.append(f"the good repository should pass, but got: {problems}")
        if count != len([name for name in GOOD if name.endswith(".md")]) - 1:
            failures.append(f"the good repository has {count} other Markdown files")

        for index, (title, change, wanted) in enumerate(SELF_TEST_CASES):
            files = dict(GOOD)
            change(files)
            case = scratch / f"case-{index}"
            _write(case, files)
            problems, _ = check(case)
            for text in wanted:
                if not any(text in problem for problem in problems):
                    failures.append(
                        f"{title}: expected a problem containing {text!r}, got {problems}"
                    )

        _scratch_bump(scratch, failures)

    if failures:
        print("The build docs self-test failed:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print(
        f"Build docs self-test passed ({len(SELF_TEST_CASES)} broken repositories caught, "
        "and a scratch wasm-bindgen bump of this repository)."
    )
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="check the checker against repositories that break each rule",
    )
    parser.add_argument(
        "--root", type=Path, default=ROOT, help="the repository root (default: this checkout)"
    )
    arguments = parser.parse_args(argv)
    if arguments.self_test:
        return self_test()

    problems, count = check(arguments.root.resolve())
    if problems:
        print(
            "BUILDING.md and the other documents disagree with the pinned versions "
            "(docs/spec/09-quality-and-delivery.md §9.4):"
        )
        for problem in problems:
            print(f"  - {problem}")
        return 1
    print(
        f"BUILDING.md's pinned versions and apt packages match their sources, and none of "
        f"{count} other Markdown files repeats them."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
