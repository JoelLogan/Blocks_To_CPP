#!/usr/bin/env python3
"""Checks that the text of a Markdown file that was split up was moved, not lost.

When a document is split or cut down (as `apps/desktop/README.md` was split into a code map
and `docs/developer-guide/`), every sentence of the old text should still be in one of the new
files, word for word or reworded, or be dropped on purpose. This script treats the old file and
the new files as bags of sentences (where a sentence went, and in which order, does not matter)
and reports each sentence of the old file as:

* moved: its words, in that order, are in one of the new files. Markdown syntax (headings, list
  markers, emphasis, backticks, table padding), line breaks, link targets and case are ignored,
  so a sentence whose links were re-pointed or whose paragraph was re-wrapped still counts;
* reworded: not word for word, but a sentence of a new file has at least `--threshold` (default
  0.75) of its words in the same order (difflib's ratio over words); the closest one is shown;
* dropped: missing, and explained in the `--dropped` file;
* missing: none of these. The check then fails.

A code block counts line by line, and a table row by row (its cells, in order). Blank lines,
table rule rows and units of fewer than three words (a heading such as "Notes") are not counted.

The `--dropped` file has one line per sentence dropped on purpose: the start of the sentence as
the report prints it (at least three words, or the whole sentence), ` | `, and why. Lines
starting with `#` are comments. An explanation that matches no missing sentence fails the check
too, so the file cannot go stale.

Run from the repository root:

    python3 tools/check-moved-text.py OLD NEW [NEW ...] [--rev REV] [--dropped FILE]
                                      [--threshold RATIO] [--verbose]

With `--rev`, OLD is read as it was at commit REV (`git show REV:OLD`), so the check runs on the
working tree after the old file was rewritten. `--verbose` also lists the reworded sentences with
their closest match, and the dropped ones with the reason. Exit status: 0 when nothing is missing;
1 when something is missing or the `--dropped` file does not fit; 2 on a usage error.

Check the checker itself: python3 tools/check-moved-text.py --self-test

Example: the desktop README split of the M2 polish wave (W6-2, recipe in
docs/developer-guide/recipes.md):

    python3 tools/check-moved-text.py apps/desktop/README.md --rev d4dfafb \\
        apps/desktop/README.md apps/desktop/e2e/README.md docs/developer-guide/*.md \\
        BUILDING.md CONTRIBUTING.md --dropped dropped.txt
"""

from __future__ import annotations

import argparse
import difflib
import io
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import TextIO

DEFAULT_THRESHOLD = 0.75
"""The least share of a sentence's words, in order, for a new sentence to be a rewording."""

MIN_WORDS = 3
"""Units with fewer words (a heading such as "Notes", a lone table cell) are not counted."""

MIN_DROPPED_WORDS = MIN_WORDS
"""The fewest words of a sentence a `--dropped` line names: a counted sentence has at least as
many, so a whole sentence can always be named, and a shorter start would name too much."""

FENCE = re.compile(r"^\s*(```|~~~)")
TABLE_RULE = re.compile(r"^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$")
HEADING = re.compile(r"^\s{0,3}#{1,6}\s+")
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s+")
QUOTE = re.compile(r"^\s*>\s?")
IMAGE_OR_LINK = re.compile(r"!?\[([^\]]*)\]\([^)]*\)")
REFERENCE_LINK = re.compile(r"\[([^\]]+)\]\[[^\]]*\]")
AUTOLINK = re.compile(r"<(https?://[^>]+)>")
HTML_COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)
# Emphasis markers at a word's edge; an underscore inside a word (B2C_LOG) stays.
EMPHASIS = re.compile(r"(?<![\w*])(\*\*|\*|__|_)(?=\S)|(?<=\S)(\*\*|\*|__|_)(?![\w*])")
SENTENCE_END = re.compile(r"(?<=[.!?])\s+(?=\S)")
TABLE_CELL_SEPARATOR = re.compile(r"(?<!\\)\|")
TYPOGRAPHY = str.maketrans(
    {
        "‘": "'",
        "’": "'",
        "“": '"',
        "”": '"',
        "–": "-",
        "—": "-",
        " ": " ",
    }
)
TRAILING_PUNCTUATION = ".,;:"
WORD_PUNCTUATION = ".,;:!?()[]\"'"
KINDS = ("moved", "reworded", "dropped", "missing")


@dataclass(frozen=True)
class Unit:
    """One counted piece of a file, normalised: a sentence, a code line or a table row."""

    text: str
    line: int


def normalise(text: str) -> str:
    """Lower case, plain quotes and dashes, single spaces, no trailing punctuation."""
    text = " ".join(text.translate(TYPOGRAPHY).lower().split())
    return text.rstrip(TRAILING_PUNCTUATION).strip()


def strip_inline(text: str) -> str:
    """Removes inline Markdown: links and images keep their text, code spans their content."""
    text = IMAGE_OR_LINK.sub(r"\1", text)
    text = REFERENCE_LINK.sub(r"\1", text)
    text = AUTOLINK.sub(r"\1", text)
    return EMPHASIS.sub("", text.replace("`", ""))


def table_cells(line: str) -> list[str]:
    """The cells of a table row, without the outer pipes."""
    row = line.strip()
    if row.startswith("|"):
        row = row[1:]
    if row.endswith("|") and not row.endswith("\\|"):
        row = row[:-1]
    return [cell.strip() for cell in TABLE_CELL_SEPARATOR.split(row)]


def units(markdown: str) -> list[Unit]:
    """Breaks a Markdown text into the units the check compares (see the module docstring)."""
    markdown = HTML_COMMENT.sub(lambda match: "\n" * match.group(0).count("\n"), markdown)
    found: list[Unit] = []
    paragraph: list[str] = []
    start = 0
    in_code = False

    def flush() -> None:
        if paragraph:
            text = strip_inline(" ".join(paragraph))
            found.extend(Unit(normalise(sentence), start) for sentence in SENTENCE_END.split(text))
            paragraph.clear()

    for number, raw in enumerate(markdown.splitlines(), start=1):
        if FENCE.match(raw):
            flush()
            in_code = not in_code
            continue
        if in_code:
            found.append(Unit(normalise(raw), number))
            continue
        line = QUOTE.sub("", raw)
        if not line.strip():
            flush()
        elif line.lstrip().startswith("|"):
            flush()
            if not TABLE_RULE.match(line):
                cells = [normalise(strip_inline(cell)) for cell in table_cells(line)]
                found.append(Unit(" | ".join(cells), number))
        elif HEADING.match(line):
            flush()
            found.append(Unit(normalise(strip_inline(HEADING.sub("", line))), number))
        else:
            if LIST_ITEM.match(line):
                flush()
                line = LIST_ITEM.sub("", line)
            if not paragraph:
                start = number
            paragraph.append(line.strip())
    flush()
    return [unit for unit in found if len(unit.text.split()) >= MIN_WORDS]


@dataclass(frozen=True)
class Target:
    """A new file: its whole normalised text (for word-for-word matches) and its units."""

    name: str
    text: str
    units: tuple[Unit, ...]


def target(name: str, markdown: str) -> Target:
    """Prepares a new file for matching."""
    found = units(markdown)
    return Target(name, "\n".join(unit.text for unit in found), tuple(found))


@dataclass(frozen=True)
class Verdict:
    """What became of one unit of the old file."""

    unit: Unit
    kind: str
    """One of KINDS."""
    where: str = ""
    """The new file it is in, or the one with the closest unit."""
    ratio: float = 0.0
    closest: str = ""


def words(text: str) -> list[str]:
    """The words of a unit for a rewording's ratio, without the punctuation around them."""
    return [word for word in (token.strip(WORD_PUNCTUATION) for token in text.split()) if word]


def closest(unit: Unit, targets: list[Target]) -> tuple[float, str, str]:
    """The new unit with the highest word ratio to `unit`: (ratio, file, text)."""
    best = (0.0, "", "")
    matcher = difflib.SequenceMatcher(autojunk=False)
    matcher.set_seq2(words(unit.text))
    for candidate_file in targets:
        for candidate in candidate_file.units:
            matcher.set_seq1(words(candidate.text))
            if matcher.real_quick_ratio() <= best[0] or matcher.quick_ratio() <= best[0]:
                continue
            ratio = matcher.ratio()
            if ratio > best[0]:
                best = (ratio, candidate_file.name, candidate.text)
    return best


def explanation(text: str, dropped: dict[str, str]) -> str | None:
    """The reason a `--dropped` line gives for the unit `text`, if one names it."""
    return next((reason for prefix, reason in dropped.items() if text.startswith(prefix)), None)


def judge(
    old: list[Unit], targets: list[Target], threshold: float, dropped: dict[str, str]
) -> list[Verdict]:
    """Classifies every unit of the old file."""
    verdicts = []
    for unit in old:
        home = next((t.name for t in targets if unit.text in t.text), None)
        if home is not None:
            verdicts.append(Verdict(unit, "moved", home))
            continue
        ratio, where, text = closest(unit, targets)
        if ratio >= threshold:
            kind = "reworded"
        elif explanation(unit.text, dropped) is not None:
            kind = "dropped"
        else:
            kind = "missing"
        verdicts.append(Verdict(unit, kind, where, ratio, text))
    return verdicts


def read_dropped(path: Path) -> tuple[dict[str, str], list[str]]:
    """The `--dropped` file: normalised sentence starts with their reasons, and its problems."""
    entries: dict[str, str] = {}
    problems = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        prefix, separator, reason = line.partition(" | ")
        prefix = normalise(prefix)
        if not separator or not reason.strip():
            problems.append(f"{path}:{number}: no reason (write the start, ' | ' and why)")
        elif len(prefix.split()) < MIN_DROPPED_WORDS:
            problems.append(
                f"{path}:{number}: name at least {MIN_DROPPED_WORDS} words of the sentence"
            )
        else:
            entries[prefix] = reason.strip()
    return entries, problems


def read_old(path: str, rev: str | None, cwd: Path | None = None) -> str:
    """The old file, from the working tree or as it was at commit `rev`."""
    if rev is None:
        return Path(cwd or ".", path).read_text(encoding="utf-8")
    result = subprocess.run(
        ["git", "show", f"{rev}:{path}"], capture_output=True, text=True, check=False, cwd=cwd
    )
    if result.returncode != 0:
        raise SystemExit(f"error: cannot read {path} at {rev}: {result.stderr.strip()}")
    return result.stdout


def report(verdicts: list[Verdict], dropped: dict[str, str], verbose: bool, out: TextIO) -> bool:
    """Prints the report; returns whether the check passed."""
    if verbose:
        for verdict in verdicts:
            if verdict.kind == "reworded":
                print(f"reworded ({verdict.ratio:.2f}, {verdict.where}), old line "
                      f"{verdict.unit.line}:\n    old: {verdict.unit.text}\n"
                      f"    new: {verdict.closest}", file=out)
        for verdict in verdicts:
            if verdict.kind == "dropped":
                print(f"dropped, old line {verdict.unit.line}: {verdict.unit.text}\n"
                      f"    why: {explanation(verdict.unit.text, dropped)}", file=out)
    for verdict in verdicts:
        if verdict.kind == "missing":
            print(f"MISSING, old line {verdict.unit.line}: {verdict.unit.text}", file=out)
            if verdict.closest:
                print(f"    closest ({verdict.ratio:.2f}, {verdict.where}): {verdict.closest}",
                      file=out)
    explained = [v.unit.text for v in verdicts if v.kind == "dropped"]
    unused = [p for p in dropped if not any(text.startswith(p) for text in explained)]
    for prefix in unused:
        print(f"UNUSED --dropped line (no missing sentence starts with it): {prefix}", file=out)
    count = {kind: sum(1 for v in verdicts if v.kind == kind) for kind in KINDS}
    print(f"{len(verdicts)} sentences: {count['moved']} moved word for word, "
          f"{count['reworded']} reworded, {count['dropped']} dropped on purpose, "
          f"{count['missing']} missing.", file=out)
    return count["missing"] == 0 and not unused


def check(
    old_text: str,
    new_files: dict[str, str],
    threshold: float,
    dropped: dict[str, str],
    verbose: bool,
    out: TextIO,
) -> bool:
    """Runs the whole check on texts; returns whether it passed."""
    targets = [target(name, text) for name, text in new_files.items()]
    return report(judge(units(old_text), targets, threshold, dropped), dropped, verbose, out)


# Self-test --------------------------------------------------------------------------------------

OLD_FIXTURE = """# The app

The editor is built from small packages ([details](../packages/README.md)). It runs in a
**Tauri** shell, on `WebKitGTK` and WebView2.

- **Saving** writes the document with the
  canonical writer, so an unchanged project saves byte for byte the same.
- Nothing is ever loaded from the network.
- `B2C_LOG=debug` adds the paths to the log.

- Windows: MSVC and WebView2
- Linux: WebKitGTK and GTK

| Path       | Contents                         |
| ---------- | -------------------------------- |
| `src/app/` | The app shell: start-up, state   |

```sh
pnpm desktop:check          # TypeScript, ESLint and Prettier
```

The old sentence that nobody kept anywhere at all.
"""

NEW_FIXTURE = """# Architecture

It runs in a Tauri shell, on WebKitGTK and WebView2. The editor is built from small
packages ([details](../../packages/README.md)).

*Saving* writes the document with the canonical writer, so an unchanged project saves byte for
byte the same.

Nothing is loaded from the network, ever. B2C_LOG=debug adds the paths to the log.

- Linux: WebKitGTK and GTK
- Windows: MSVC and WebView2

| Path | Contents |
| --- | --- |
| `src/app/` | The app shell: start-up, state |

```sh
pnpm desktop:check   # TypeScript, ESLint and Prettier
```
"""

# What each unit of OLD_FIXTURE becomes against NEW_FIXTURE, by its first 30 characters.
EXPECTED_KINDS = {
    "the editor is built from small": "moved",  # a link re-pointed
    "it runs in a tauri shell, on w": "moved",  # emphasis and backticks ignored
    "saving writes the document wit": "moved",  # a list item re-wrapped as a paragraph
    "nothing is ever loaded from th": "reworded",
    "b2c_log=debug adds the paths t": "moved",  # an underscore inside a word kept
    "windows: msvc and webview2": "moved",  # list items without a full stop, in another order
    "linux: webkitgtk and gtk": "moved",
    "path | contents": "moved",  # a table row re-padded
    "src/app/ | the app shell: star": "moved",
    "pnpm desktop:check # typescrip": "moved",  # a code line re-spaced
    "the old sentence that nobody k": "missing",
}


def self_test() -> int:
    """Checks the script on synthetic files; returns the exit status."""
    failures = []

    def expect(name: str, condition: bool) -> None:
        if not condition:
            failures.append(name)

    new = {"new.md": NEW_FIXTURE}
    verdicts = judge(units(OLD_FIXTURE), [target("new.md", NEW_FIXTURE)], DEFAULT_THRESHOLD, {})
    kinds = {verdict.unit.text[:30]: verdict.kind for verdict in verdicts}
    for start, kind in EXPECTED_KINDS.items():
        expect(f"'{start}…' is {kind} (got {kinds.get(start)})", kinds.get(start) == kind)
    expect("a heading of two words is not counted", len(kinds) == len(EXPECTED_KINDS))

    sink = io.StringIO()
    explained = {"the old sentence that nobody kept": "covered elsewhere"}
    expect("a missing sentence fails the check",
           not check(OLD_FIXTURE, new, DEFAULT_THRESHOLD, {}, False, sink))
    expect("the report names the missing sentence",
           "MISSING, old line 22: the old sentence" in sink.getvalue())
    expect("an explained sentence passes",
           check(OLD_FIXTURE, new, DEFAULT_THRESHOLD, explained, True, sink))
    stale = {**explained, "a sentence that is not in the old file": "stale"}
    expect("an unused explanation fails the check",
           not check(OLD_FIXTURE, new, DEFAULT_THRESHOLD, stale, False, sink))
    expect("a high threshold turns a rewording into a missing sentence",
           not check(OLD_FIXTURE, new, 0.99, explained, False, sink))

    with tempfile.TemporaryDirectory() as folder:
        dropped_file = Path(folder, "dropped.txt")
        dropped_file.write_text(
            "# a comment\n"
            "The old sentence that nobody kept | covered elsewhere\n"
            "On Debian or Ubuntu: | a whole short sentence\n"
            "too short | a reason\n"
            "a sentence start without a reason\n",
            encoding="utf-8",
        )
        entries, problems = read_dropped(dropped_file)
        expect("--dropped lines are read, normalised",
               entries == {"the old sentence that nobody kept": "covered elsewhere",
                           "on debian or ubuntu": "a whole short sentence"})
        expect("a short or unexplained --dropped line is a problem", len(problems) == 2)

        repo = Path(folder, "repo")
        repo.mkdir()
        git = ["git", "-C", str(repo), "-c", "user.name=self-test", "-c", "user.email=self@test"]
        subprocess.run([*git, "init", "--quiet"], check=True)
        Path(repo, "old.md").write_text(OLD_FIXTURE, encoding="utf-8")
        subprocess.run([*git, "add", "old.md"], check=True)
        subprocess.run([*git, "commit", "--quiet", "--no-gpg-sign", "-m", "old"], check=True)
        Path(repo, "old.md").write_text("Rewritten.\n", encoding="utf-8")
        subprocess.run([*git, "commit", "--quiet", "--no-gpg-sign", "-am", "new"], check=True)
        Path(repo, "old.md").write_text("Rewritten again.\n", encoding="utf-8")
        expect("--rev reads the file as it was at that commit",
               read_old("old.md", "HEAD~1", cwd=repo) == OLD_FIXTURE)
        expect("without --rev the working tree is read",
               read_old("old.md", None, cwd=repo) == "Rewritten again.\n")

    if failures:
        print("The check-moved-text self-test failed:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print(f"check-moved-text self-test passed ({len(EXPECTED_KINDS)} fixture sentences, "
          "--dropped and --rev).")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description=(__doc__ or "").split("\n\n")[0],
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("old", nargs="?", help="the Markdown file whose text was moved")
    parser.add_argument("new", nargs="*", help="the Markdown files the text was moved to")
    parser.add_argument("--rev", help="read OLD as it was at this commit (git show REV:OLD)")
    parser.add_argument("--dropped", type=Path,
                        help="sentences dropped on purpose, one 'start of the sentence | why' a line")
    parser.add_argument("--threshold", type=float, default=DEFAULT_THRESHOLD,
                        help="the least word ratio of a rewording (default %(default)s)")
    parser.add_argument("--verbose", action="store_true",
                        help="also list the reworded and the dropped sentences")
    parser.add_argument("--self-test", action="store_true",
                        help="check this script on synthetic files")
    # Intermixed, so that options may come between OLD and the NEW files.
    args = parser.parse_intermixed_args(argv)
    if args.self_test:
        return self_test()
    if args.old is None or not args.new:
        parser.error("give the old file and at least one new file")
    if not 0.0 < args.threshold <= 1.0:
        parser.error("--threshold must be above 0 and at most 1")
    dropped: dict[str, str] = {}
    if args.dropped is not None:
        dropped, problems = read_dropped(args.dropped)
        if problems:
            print("\n".join(problems))
            return 1
    new_files = {name: Path(name).read_text(encoding="utf-8") for name in args.new}
    old_text = read_old(args.old, args.rev)
    return 0 if check(old_text, new_files, args.threshold, dropped, args.verbose, sys.stdout) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
