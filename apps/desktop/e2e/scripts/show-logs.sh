#!/usr/bin/env bash
# Prints the logs the end-to-end tests and the benchmarks left in their artifacts folder, for a
# CI job that failed (the artifacts themselves are uploaded too, but the job's log is what is read
# first):
#
#   apps/desktop/e2e/scripts/show-logs.sh <artifacts folder>
#
# - every .log and .txt file: its last 300 lines;
# - a failed test's part of the native driver's log (native-driver.log, written when
#   B2C_E2E_NATIVE_DRIVER_LOG is set; see e2e/README.md): only the entries that tell what happened
#   (WebDriver commands and responses, console messages, script exceptions, navigations and
#   closed targets, warnings), last 2,500 lines. msedgedriver's verbose log is too long otherwise.
set -euo pipefail

artifacts=${1:?usage: show-logs.sh <artifacts folder>}
if [ ! -d "$artifacts" ]; then
  exit 0
fi

# The header of an entry of msedgedriver's log: "[<seconds>.<ms>][<LEVEL>]: …"; the lines after it
# up to the next header belong to it.
entries='COMMAND|RESPONSE|consoleAPICalled|Log\.entryAdded|exceptionThrown|frameNavigated|targetDestroyed|detachedFromTarget|Inspector\.detached|\]\[(WARNING|SEVERE)\]'

find "$artifacts" -type f \( -name '*.log' -o -name '*.txt' \) -print0 | sort -z |
  while IFS= read -r -d '' file; do
    echo "::group::$file"
    if [ "$(basename "$file")" = native-driver.log ]; then
      awk -v pattern="$entries" '
        function flush() {
          if (keep && entry != "") print entry
          entry = ""
        }
        /^\[[0-9]+\.[0-9]+\]\[[A-Z]+\]/ { flush(); keep = ($0 ~ pattern); entry = $0; next }
        { if (entry != "") entry = entry "\n" $0 }
        END { flush() }
      ' "$file" | tail -n 2500
    else
      tail -n 300 "$file"
    fi
    echo "::endgroup::"
  done
