#!/usr/bin/env python3
"""Checks that every diagnostic code used in the Rust sources is documented.

Spec docs/spec/06-compiler-pipeline.md §6.12: every code has an entry in
docs/reference/diagnostics/. A code counts as documented when it appears in a
Markdown file in that folder. Codes look like B2C-E0201, B2C-W0510, B2C-I0513
or B2C-T1001.

Exit status 1 lists undocumented codes; documented codes that are no longer
used are reported as warnings.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CODE = re.compile(r"\bB2C-[EWIT]\d{4}\b")


def codes_in(paths):
    found = {}
    for path in paths:
        for match in CODE.finditer(path.read_text(encoding="utf-8")):
            found.setdefault(match.group(0), path.relative_to(ROOT))
    return found


def main():
    sources = [p for p in (ROOT / "crates").rglob("*.rs") if "target" not in p.parts]
    docs = list((ROOT / "docs" / "reference" / "diagnostics").glob("*.md"))
    used = codes_in(sources)
    documented = codes_in(docs)
    missing = sorted(set(used) - set(documented))
    unused = sorted(set(documented) - set(used))
    for code in unused:
        print(f"warning: {code} is documented but not used in crates/")
    if missing:
        print("error: these diagnostic codes have no entry in docs/reference/diagnostics/:")
        for code in missing:
            print(f"  {code} (first used in {used[code]})")
        return 1
    print(f"All {len(used)} diagnostic codes are documented.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
