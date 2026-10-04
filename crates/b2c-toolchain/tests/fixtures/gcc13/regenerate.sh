#!/bin/sh
# Regenerates the GCC 13 diagnostics fixtures in this folder from src/.
# Usage: sh crates/b2c-toolchain/tests/fixtures/gcc13/regenerate.sh [g++]
# Each source is compiled three times, once per diagnostics format of the
# ladder in docs/spec/07-toolchain-build-run.md §7.5.3:
#   <name>.plain.txt  stderr with -fdiagnostics-plain-output
#   <name>.json       stderr with -fdiagnostics-format=json
#   <name>.sarif      the file written by -fdiagnostics-format=sarif-file
# link.cpp is also linked, so its outputs include the linker's messages.
# Absolute paths are replaced by /work so the fixtures do not depend on
# where the repository is checked out.
set -u
here=$(cd "$(dirname "$0")" && pwd)
gxx=${1:-g++}
cd "$here" || exit 1
common="-std=c++20 -fdiagnostics-color=never -fdiagnostics-urls=never -fmessage-length=0 -Wall -Wextra -Wpedantic"
strict="-Wshadow -Wconversion -Wsign-conversion -Wdouble-promotion"
"$gxx" --version | head -n 1 > compiler-version.txt

for name in errors template fatal strict link; do
    flags=$common
    [ "$name" = strict ] && flags="$common $strict"
    if [ "$name" = link ]; then step="-o /dev/null"; else step="-fsyntax-only"; fi
    # shellcheck disable=SC2086 # word splitting of the flag lists is intended
    LC_ALL=C "$gxx" $flags -fdiagnostics-plain-output $step "src/$name.cpp" 2> "$name.plain.txt"
    # shellcheck disable=SC2086
    LC_ALL=C "$gxx" $flags -fdiagnostics-format=json $step "src/$name.cpp" 2> "$name.json"
    # shellcheck disable=SC2086
    LC_ALL=C "$gxx" $flags -fdiagnostics-format=sarif-file $step "src/$name.cpp" 2> "$name.sarif-stderr.txt"
    mv "$name.cpp.sarif" "$name.sarif"
done
for file in *.txt *.json *.sarif; do
    sed -i "s#$here#/work#g" "$file"
done
