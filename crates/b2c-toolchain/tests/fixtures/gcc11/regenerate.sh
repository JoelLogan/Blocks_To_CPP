#!/bin/sh
# Regenerates the GCC 11 diagnostics fixtures in this folder.
# Usage: sh crates/b2c-toolchain/tests/fixtures/gcc11/regenerate.sh [g++-11]
#
# GCC 11 is the oldest supported compiler (b2c_toolchain::probe::MIN_GCC_MAJOR).
# It has no SARIF output, so only two formats of the ladder in
# docs/spec/07-toolchain-build-run.md §7.5.3 are recorded:
#   <name>.plain.txt  stderr with -fdiagnostics-plain-output
#   <name>.json       stderr with -fdiagnostics-format=json
# The sources are the GCC 13 fixtures' (../gcc13/src), compiled from that
# folder so file names read `src/<name>.cpp` as in the GCC 13 fixtures.
# link.cpp is also linked, so its outputs include the linker's messages.
# Absolute paths are replaced by /work so the fixtures do not depend on
# where the repository is checked out. The fuzz targets (fuzz/README.md)
# use these files as seeds.
set -u
here=$(cd "$(dirname "$0")" && pwd)
sources=$(cd "$here/../gcc13" && pwd)
gxx=${1:-g++-11}

version=$("$gxx" --version | head -n 1)
case "$version" in
*" 11."*) ;;
*)
    echo "regenerate.sh: $gxx is not GCC 11: $version" >&2
    exit 1
    ;;
esac
printf '%s\n' "$version" > "$here/compiler-version.txt"

common="-std=c++20 -fdiagnostics-color=never -fdiagnostics-urls=never -fmessage-length=0 -Wall -Wextra -Wpedantic"
strict="-Wshadow -Wconversion -Wsign-conversion -Wdouble-promotion"
cd "$sources" || exit 1
for name in errors template fatal strict link; do
    flags=$common
    [ "$name" = strict ] && flags="$common $strict"
    if [ "$name" = link ]; then step="-o /dev/null"; else step="-fsyntax-only"; fi
    # shellcheck disable=SC2086 # word splitting of the flag lists is intended
    LC_ALL=C "$gxx" $flags -fdiagnostics-plain-output $step "src/$name.cpp" 2> "$here/$name.plain.txt"
    # shellcheck disable=SC2086
    LC_ALL=C "$gxx" $flags -fdiagnostics-format=json $step "src/$name.cpp" 2> "$here/$name.json"
done
cd "$here" || exit 1
for file in *.txt *.json; do
    sed -i -e "s#$sources#/work#g" -e "s#$here#/work#g" "$file"
done
