#!/usr/bin/env python3
"""Writes BUILDING.md's Debian and Ubuntu steps as one shell script.

M3-7 (docs/plans/m3-contracts/w5-performance.md §8): the weekly job of
.github/workflows/building.yml follows BUILDING.md's section "Linux: Debian
and Ubuntu" word for word in Debian 12 and 13 containers. This script takes
the section's shell blocks, in order, and makes exactly these changes, each
printed to standard error so the job's log shows them:

* `sudo ` is dropped: the container runs as root and has no sudo;
* `git clone <url>` and the `cd` into its folder that follows are replaced by
  a `cd` into the checkout the job already made (passed as --checkout), so
  the job builds the commit it runs for;
* the last step, which starts the app, is replaced by a check that the app
  is still running after 20 seconds under xvfb (as desktop.yml's start check).

Anything else in the blocks runs as written. The script fails when the
section, its blocks or one of the lines it changes is missing, so a reworded
BUILDING.md is noticed rather than half followed.

Usage: building-steps.py --checkout DIR [--building BUILDING.md] > steps.sh
       building-steps.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

SECTION = "## Linux: Debian and Ubuntu"
START = "./target/release/blocks2cpp-desktop"
START_CHECK = """status=0
timeout 20 xvfb-run --auto-servernum ./target/release/blocks2cpp-desktop \\
  > app.log 2>&1 || status=$?
cat app.log
# 124: still running when the time ran out, which is what is wanted.
if [ "$status" -ne 124 ]; then
  echo "The app exited with status $status within 20 seconds" >&2
  exit 1
fi"""


class StepsError(Exception):
    """BUILDING.md does not have what the job follows."""


def section_blocks(text: str) -> list[str]:
    """The `sh` blocks of the Debian and Ubuntu section, in order."""
    start = text.find(f"\n{SECTION}\n")
    if start < 0:
        raise StepsError(f"BUILDING.md has no section {SECTION!r}")
    rest = text[start + len(SECTION) + 2 :]
    end = re.search(r"^#{2,3} ", rest, re.MULTILINE)
    body = rest if end is None else rest[: end.start()]
    blocks = re.findall(r"^ *```sh\n(.*?)^ *```$", body, re.MULTILINE | re.DOTALL)
    if not blocks:
        raise StepsError("the Debian and Ubuntu section has no sh blocks")
    # The blocks are indented under their numbered steps.
    return ["\n".join(line.strip() for line in block.splitlines()) for block in blocks]


def script(text: str, checkout: str) -> tuple[str, list[str]]:
    """The steps as one script, and the changes made."""
    changes: list[str] = []
    lines: list[str] = ["set -euxo pipefail"]
    blocks = section_blocks(text)
    cloned = started = False
    skip_cd = False
    for block in blocks:
        for line in block.splitlines():
            if skip_cd and line.startswith("cd ") and line != "cd -":
                changes.append(f"replaced {line!r} by the checkout")
                skip_cd = False
                continue
            if line.startswith("git clone "):
                lines.append(f"cd {quote(checkout)}")
                changes.append(f"replaced {line!r} by cd into the checkout")
                cloned = skip_cd = True
                continue
            if line == START:
                lines.append(START_CHECK)
                changes.append(f"replaced {line!r} by a 20-second start under xvfb")
                started = True
                continue
            if line.startswith("sudo "):
                changes.append(f"dropped sudo from {line!r}")
                line = line[len("sudo ") :]
            lines.append(line)
    if not cloned:
        raise StepsError("the section has no git clone step")
    if not started:
        raise StepsError(f"the section does not start the app with {START!r}")
    return "\n".join(lines) + "\n", changes


def quote(path: str) -> str:
    """`path` quoted for the shell."""
    return "'" + path.replace("'", "'\\''") + "'"


SAMPLE = """# Building

## Linux: Debian and Ubuntu

1. Packages.

   ```sh
   sudo apt update
   sudo apt install a \\
     b
   ```

2. Source.

   ```sh
   git clone https://example.org/x.git
   cd x
   pnpm app:build
   ```

3. Start it:

   ```sh
   ./target/release/blocks2cpp-desktop
   ```

### Fedora (untested)

```sh
sudo dnf install c
```
"""


def self_test() -> int:
    """Checks the extraction on a sample and on the repository's BUILDING.md."""
    steps, changes = script(SAMPLE, "/w/it's")
    expected_lines = [
        "set -euxo pipefail",
        "apt update",
        "apt install a \\",
        "b",
        "cd '/w/it'\\''s'",
        "pnpm app:build",
    ]
    assert steps.splitlines()[: len(expected_lines)] == expected_lines, steps
    assert "timeout 20 xvfb-run" in steps and "dnf" not in steps and "git clone" not in steps, steps
    assert len(changes) == 5, changes
    for broken, reason in [
        (SAMPLE.replace("## Linux: Debian and Ubuntu", "## Linux"), "no section"),
        (SAMPLE.replace("git clone", "git fetch"), "no git clone"),
        (SAMPLE.replace("./target/release/", "./target/debug/"), "does not start"),
    ]:
        try:
            script(broken, "/w")
        except StepsError as error:
            assert reason in str(error), error
        else:
            raise AssertionError(f"accepted a BUILDING.md with {reason}")
    real = Path(__file__).resolve().parent.parent / "BUILDING.md"
    steps, _ = script(real.read_text(encoding="utf-8"), "/w")
    assert "sudo" not in steps and "pnpm app:build" in steps, steps
    print("building-steps: self-test passed")
    return 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--checkout", required=True, help="the repository's checkout to build")
    parser.add_argument(
        "--building",
        default=str(Path(__file__).resolve().parent.parent / "BUILDING.md"),
        help="BUILDING.md (by default the repository's)",
    )
    options = parser.parse_args(argv)
    try:
        text = Path(options.building).read_text(encoding="utf-8")
        steps, changes = script(text, options.checkout)
    except (OSError, StepsError) as error:
        print(f"building-steps: {error}", file=sys.stderr)
        return 1
    for change in changes:
        print(f"building-steps: {change}", file=sys.stderr)
    sys.stdout.write(steps)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
