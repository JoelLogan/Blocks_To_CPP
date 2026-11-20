#!/usr/bin/env python3
"""Generates crates/b2c-ir/src/std_names.rs, the standard-name table, from g++.

The table (docs/spec/06-compiler-pipeline.md §6.14.8,
docs/plans/m3-contracts/w3-modules.md §2.12) lists every name that the
namespace `std` declares, over every GCC it was made from, C++17 to C++26,
strict and GNU modes and every standard header (`<bits/stdc++.h>`), with the
kind of each name (namespace, type, class template, object, function) and the
first standard that has it. Code generation shortens a standard name, or
leaves a user's name unqualified, only where the table says no standard name
is spelled like it; the names of types and namespaces of the project's
standard also give B2C-E0215.

Names are found by compile probes, never by parsing headers: each identifier
of the preprocessed headers is tried as `using std::NAME;`, and each name
that follows `namespace` as `namespace a = std::NAME;`; a probe that the
compiler refuses, or crashes on, counts as "no". Hidden friends cannot be
probed, so every identifier that a `friend` declaration of the headers names
before `(` is listed as a function too (a superset, which can only add a
harmless `::`).
Names that no user may choose (with `__`, or not starting with a letter) are
left out.

Usage, from the repository root:

  python3 tools/gen-std-names.py [--compiler G++]... [--merge LIST]...
      Writes the table from the compilers given, by default every g++-N on
      PATH from GCC 11 (b2c_toolchain::probe::MIN_GCC_MAJOR) on, or g++ when
      there is none, plus the lists given (see --print).
  python3 tools/gen-std-names.py --check [--compiler G++]... [--print]
      Probes the compilers (by default g++) and fails, naming them, when the
      table lacks a name they declare. With --print, also prints what they
      declare as a list for --merge: the CI job with the newest GCC prints
      the list the table is regenerated with (only GCC 11 to 14 are local).

The output is deterministic for given compilers and lists; review the diff
when regenerating.
"""

import argparse
import concurrent.futures
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

OUT = Path("crates/b2c-ir/src/std_names.rs")
MIN_GCC_MAJOR = 11
# The standards, by the year the table records, with the spellings g++
# accepts for each (an older g++ knows a newer standard by its draft name).
STANDARDS = [
    (17, ["c++17"]),
    (20, ["c++20", "c++2a"]),
    (23, ["c++23", "c++2b"]),
    (26, ["c++26", "c++2c"]),
]
# The kinds, from the most type-like: a name that is a type in one standard
# or mode and something else in another is recorded as the first that fits.
KINDS = ["namespace", "type", "class_template", "object", "function"]
USER_NAME = re.compile(r"^[A-Za-z][A-Za-z0-9_]*$")
KEYWORDS = {
    "alignas", "alignof", "and", "and_eq", "asm", "auto", "bitand", "bitor", "bool", "break", "case",
    "catch", "char", "char8_t", "char16_t", "char32_t", "class", "compl", "concept", "const", "consteval",
    "constexpr", "constinit", "const_cast", "continue", "co_await", "co_return", "co_yield", "decltype",
    "default", "delete", "do", "double", "dynamic_cast", "else", "enum", "explicit", "export", "extern",
    "false", "float", "for", "friend", "goto", "if", "inline", "int", "long", "mutable", "namespace",
    "new", "noexcept", "not", "not_eq", "nullptr", "operator", "or", "or_eq", "private", "protected",
    "public", "register", "reinterpret_cast", "requires", "return", "short", "signed", "sizeof", "static",
    "static_assert", "static_cast", "struct", "switch", "template", "this", "thread_local", "throw",
    "true", "try", "typedef", "typeid", "typename", "union", "unsigned", "using", "virtual", "void",
    "volatile", "wchar_t", "while", "xor", "xor_eq",
}
PROBE_HEADER = "#include <bits/stdc++.h>\n"
# What declares a kind, tried on the names `using std::NAME;` accepts.
KIND_PROBES = [
    ("type", "", "using b2c_t_{i} = std::{name};"),
    (
        "class_template",
        "template <template <class...> class> struct b2c_tt {};",
        "using b2c_c_{i} = b2c_tt<std::{name}>;",
    ),
    (
        "class_template",
        "template <template <auto...> class> struct b2c_tt {};",
        "using b2c_c_{i} = b2c_tt<std::{name}>;",
    ),
    (
        "class_template",
        "template <template <class, auto...> class> struct b2c_tt {};",
        "using b2c_c_{i} = b2c_tt<std::{name}>;",
    ),
    ("object", "", "static_assert(!std::is_function<decltype(std::{name})>::value, \"\");"),
]


def run(args, stdin=""):
    return subprocess.run(args, input=stdin, capture_output=True, text=True, check=False)


def version_of(gxx):
    full = run([gxx, "-dumpfullversion"]).stdout.strip()
    target = run([gxx, "-dumpmachine"]).stdout.strip()
    return full, target


def standards_of(gxx):
    """The standards g++ takes, each with the spelling it takes."""
    found = []
    for year, spellings in STANDARDS:
        for spelling in spellings:
            if run([gxx, f"-std={spelling}", "-fsyntax-only", "-x", "c++", "-"], "").returncode == 0:
                found.append((year, spelling))
                break
    return found


def failed_lines(gxx, flags, lines, name):
    """The lines of a probe file that g++ refuses (1-based), and the line it
    crashed on, or None. Some g++ crash on some probes (GCC 13 on a concept
    given as a template template argument); the crash ends the run, so the
    lines after it were never tried."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / name
        path.write_text("\n".join(lines) + "\n")
        res = run([gxx, *flags, "-fsyntax-only", "-fmax-errors=0", "-w", str(path)])
    pattern = re.compile(re.escape(name) + r":(\d+):\d+: (?:fatal )?error")
    refused = {int(m.group(1)) for m in pattern.finditer(res.stderr)}
    crashed = (
        res.returncode not in (0, 1)
        or "internal compiler error" in res.stderr
        or "confused by earlier errors" in res.stderr
    )
    if not crashed:
        return refused, None
    mentioned = [int(m.group(1)) for m in re.finditer(re.escape(name) + r":(\d+):", res.stderr)]
    return refused, max(mentioned, default=0)


def accepted(gxx, flags, names, preamble, template, file_name):
    """The names whose probe line g++ accepts. A name g++ crashes on counts
    as refused, and the names after it are tried again in a new run."""
    found = set()
    start = 0
    while start < len(names):
        lines = [PROBE_HEADER.strip()]
        if preamble:
            lines.append(preamble)
        first = len(lines) + 1
        chunk = names[start:]
        lines += [template.format(i=start + i, name=name) for i, name in enumerate(chunk)]
        refused, crashed_on = failed_lines(gxx, flags, lines, file_name)
        if first - 1 in refused or 1 in refused:
            raise SystemExit(f"{gxx} {' '.join(flags)}: the probe's preamble does not compile")
        tried = len(chunk) if crashed_on is None else max(crashed_on - first, 0)
        found |= {name for i, name in enumerate(chunk[:tried]) if first + i not in refused}
        if crashed_on is None:
            break
        start += tried + 1
    return found


def probe(gxx, year, spelling, gnu, with_kinds):
    """What `std` declares under one standard and mode: {name: kind}."""
    flags = [f"-std={spelling.replace('c++', 'gnu++')}" if gnu else f"-std={spelling}"]
    pre = run([gxx, *flags, "-E", "-P", "-x", "c++", "-"], PROBE_HEADER)
    if pre.returncode != 0:
        raise SystemExit(f"{gxx} {flags[0]}: the standard headers do not preprocess:\n{pre.stderr}")
    text = pre.stdout
    words = {w for w in re.findall(r"\b[A-Za-z][A-Za-z0-9_]*\b", text) if "__" not in w}
    words -= KEYWORDS
    candidates = sorted(words)
    declared = accepted(gxx, flags, candidates, "", "namespace b2c_p_{i} {{ using std::{name}; }}", "names.cpp")
    namespaces = sorted({n for n in re.findall(r"\bnamespace\s+([A-Za-z][A-Za-z0-9_]*)\b", text) if "__" not in n})
    namespaces = accepted(gxx, flags, namespaces, "", "namespace b2c_a_{i} = std::{name};", "spaces.cpp")
    friends = {
        n
        for n in re.findall(r"\bfriend\b[^;{}]*?\b([A-Za-z][A-Za-z0-9_]*)\s*\(", text)
        if USER_NAME.match(n) and "__" not in n and n not in KEYWORDS
    }
    table = {name: "namespace" for name in namespaces}
    rest = sorted(declared - namespaces)
    if with_kinds:
        for kind, preamble, template in KIND_PROBES:
            open_names = [name for name in rest if name not in table]
            for name in accepted(gxx, flags, open_names, preamble, template, f"{kind}.cpp"):
                table[name] = kind
    for name in rest:
        table.setdefault(name, "function")
    for name in friends:
        table.setdefault(name, "function")
    return year, table


def probe_compilers(compilers, with_kinds):
    """{name: (kind, first year)} over every compiler, standard and mode."""
    jobs = []
    for gxx in compilers:
        for year, spelling in standards_of(gxx):
            for gnu in (False, True):
                jobs.append((gxx, year, spelling, gnu))
    workers = max(1, min(4, (os.cpu_count() or 2) // 2))
    merged = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        results = pool.map(lambda job: probe(*job, with_kinds), jobs)
        for year, table in results:
            merge_into(merged, table, year)
    return merged


def merge_into(merged, table, year):
    for name, kind in table.items():
        old = merged.get(name)
        if old is None:
            merged[name] = (kind, year)
        else:
            best = min(old[0], kind, key=KINDS.index)
            merged[name] = (best, min(old[1], year))


def read_list(path):
    """A list --print wrote: its source line and {name: (kind, year)}."""
    source = None
    table = {}
    for line in Path(path).read_text().splitlines():
        if line.startswith("# "):
            source = source or line[2:].strip()
            continue
        if not line.strip():
            continue
        name, kind, year = line.split()
        if not USER_NAME.match(name) or kind not in KINDS or int(year) not in dict(STANDARDS):
            raise SystemExit(f"{path}: not a line of a name list: {line!r}")
        table[name] = (kind, int(year))
    if source is None:
        raise SystemExit(f"{path}: the list does not say which g++ it came from")
    return source, table


def write_list(sources, table):
    lines = [f"# {source}" for source in sources]
    lines += [f"{name} {kind} {year}" for name, (kind, year) in sorted(table.items())]
    return "\n".join(lines) + "\n"


def read_table():
    """The checked-in table's names."""
    text = OUT.read_text()
    return set(re.findall(r'^    n\("([A-Za-z0-9_]+)",', text, re.MULTILINE))


def rust_kind(kind):
    return "".join(part.capitalize() for part in kind.split("_"))


def write_table(sources, table):
    majors = sorted({int(re.match(r"GCC (\d+)", s).group(1)) for s in sources})
    body = "\n".join(
        f'    n("{name}", StdNameKind::{rust_kind(kind)}, {year}),' for name, (kind, year) in sorted(table.items())
    )
    source_lines = "\n".join(f"// - {source}" for source in sources)
    gcc_lines = "\n".join(f'    "{source}",' for source in sources)
    text = f"""// @generated by tools/gen-std-names.py — do not edit by hand.
// Made from:
{source_lines}

//! The standard-name table (06 §6.14.8, w3-modules.md §2.12): every name the
//! namespace `std` declares in the GCC versions it was made from
//! ([`STD_NAMES_GCC`]), C++17 to C++26, strict and GNU modes, every standard
//! header, with its kind and the first standard that has it. A toolchain
//! newer than [`NEWEST_GCC_MAJOR`] counts as an incomplete table (06
//! §6.14.8, w3-modules.md §1.8).

/// The GCC versions the table was made from, oldest first.
pub const STD_NAMES_GCC: &[&str] = &[
{gcc_lines}
];

/// The newest major version of GCC the table was made from.
pub const NEWEST_GCC_MAJOR: u32 = {majors[-1]};

/// What a standard name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StdNameKind {{
    /// A namespace (`std::chrono`).
    Namespace,
    /// A type (`std::string`, `std::size_t`).
    Type,
    /// A class or alias template (`std::vector`, `std::array`).
    ClassTemplate,
    /// An object (`std::cout`, `std::nullopt`).
    Object,
    /// A function, a function template, a variable template or a hidden
    /// friend (`std::max`, `std::swap`): anything else `using std::NAME;`
    /// accepts.
    Function,
}}

/// One name of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StdName {{
    /// The name, as `std::` declares it.
    pub name: &'static str,
    /// What it is; when it differs between standards or modes, the most
    /// type-like of them.
    pub kind: StdNameKind,
    /// The first standard that has it: 17, 20, 23 or 26.
    pub since: u8,
}}

impl StdNameKind {{
    /// Whether the name is a type or a namespace, whose names `B2C-E0215`
    /// checks against the project's standard (06 §6.14.7).
    pub const fn is_type_or_namespace(self) -> bool {{
        matches!(self, Self::Namespace | Self::Type | Self::ClassTemplate)
    }}
}}

/// The entry for `name`, if `std` declares it in any standard.
pub fn std_name(name: &str) -> Option<&'static StdName> {{
    STD_NAMES
        .binary_search_by(|entry| entry.name.cmp(name))
        .ok()
        .and_then(|index| STD_NAMES.get(index))
}}

const fn n(name: &'static str, kind: StdNameKind, since: u8) -> StdName {{
    StdName {{ name, kind, since }}
}}

/// Every name of `std`, sorted by name (byte order) for binary search.
#[rustfmt::skip]
pub const STD_NAMES: &[StdName] = &[
{body}
];
"""
    OUT.write_text(text)


def default_compilers():
    found = {}
    for directory in os.environ.get("PATH", "").split(os.pathsep):
        if not directory or not os.path.isdir(directory):
            continue
        for entry in sorted(os.listdir(directory)):
            match = re.fullmatch(r"g\+\+-(\d+)", entry)
            if match and int(match.group(1)) >= MIN_GCC_MAJOR:
                path = os.path.join(directory, entry)
                full, _ = version_of(path)
                if full and full not in found:
                    found[full] = path
    if not found:
        return ["g++"]
    return [found[version] for version in sorted(found, key=lambda v: [int(p) for p in v.split(".")])]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--compiler", action="append", default=[], help="a g++ to probe (repeatable)")
    parser.add_argument("--merge", action="append", default=[], help="a list --print wrote (repeatable)")
    parser.add_argument("--check", action="store_true", help="check the table instead of writing it")
    parser.add_argument("--print", action="store_true", help="with --check, print the probed list")
    args = parser.parse_args()
    if args.check:
        compilers = args.compiler or ["g++"]
        if shutil.which(compilers[0]) is None and not os.path.exists(compilers[0]):
            raise SystemExit(f"{compilers[0]} was not found")
        probed = probe_compilers(compilers, with_kinds=args.print)
        sources = [f"GCC {version_of(gxx)[0]} ({version_of(gxx)[1]})" for gxx in compilers]
        if args.print:
            sys.stdout.write(write_list(sources, probed))
        missing = sorted(set(probed) - read_table())
        if missing:
            print(
                f"{OUT} lacks {len(missing)} name(s) that {', '.join(sources)} declare(s) in std: "
                + " ".join(missing)
                + "\nRegenerate it with tools/gen-std-names.py (with --merge for a GCC that is not local).",
                file=sys.stderr,
            )
            sys.exit(1)
        print(f"{OUT} has every name {', '.join(sources)} declare(s) in std ({len(probed)}).", file=sys.stderr)
        return
    compilers = args.compiler or default_compilers()
    table = probe_compilers(compilers, with_kinds=True)
    sources = [f"GCC {version_of(gxx)[0]} ({version_of(gxx)[1]})" for gxx in compilers]
    for path in args.merge:
        source, listed = read_list(path)
        sources.append(source)
        for name, (kind, year) in listed.items():
            merge_into(table, {name: kind}, year)
    sources = sorted(set(sources), key=lambda s: [int(p) for p in re.match(r"GCC ([\d.]+)", s).group(1).split(".")])
    write_table(sources, table)
    print(f"{OUT}: {len(table)} names from {', '.join(sources)}")


if __name__ == "__main__":
    main()
