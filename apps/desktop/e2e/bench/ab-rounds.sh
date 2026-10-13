#!/usr/bin/env bash
# The A/B benchmark runs (docs/spec/09-quality-and-delivery.md §9.2, README.md "A/B runs"): the
# webview benchmarks of this checkout run against two builds of the app on one machine, in
# alternating runs, so that both builds meet the same machine state; each run writes its results
# files into <out>/<base|head>/round-<n>/, and tools/bench-compare.py ab adds up each side's
# samples and compares the medians.
#
# Usage: ab-rounds.sh <base app> <head app> <out folder> <benchmark file filter>...
#   e.g. ab-rounds.sh /tmp/base/blocks2cpp-desktop /tmp/head/blocks2cpp-desktop /tmp/ab \
#          preview.bench select.bench
#
# Environment:
#   AB_ROUNDS   rounds (default 2); round n runs the base first when n is odd, the head first
#               otherwise (base, head, head, base, ...)
#   AB_SAMPLES  samples per benchmark and run (default 5; B2C_BENCH_SAMPLES of the runs). Round n
#               starts the benchmarks that go round a list of targets (the selection ones) at
#               (n - 1) * AB_SAMPLES (B2C_BENCH_OFFSET), the same for both builds, so the rounds
#               together click the targets a nightly run does
#   AB_XVFB     1: run each benchmark run under xvfb-run (Linux CI)
# The rest of the harness's settings (e2e/README.md) are passed through.
set -euo pipefail

if [ "$#" -lt 4 ]; then
  echo "usage: $0 <base app> <head app> <out folder> <benchmark file filter>..." >&2
  exit 2
fi
# The harness needs absolute paths (e2e/README.md "Settings").
absolute() {
  case "$1" in
    /* | [A-Za-z]:[\\/]*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$PWD" "$1" ;;
  esac
}
base_app=$(absolute "$1")
head_app=$(absolute "$2")
out=$(absolute "$3")
shift 3

rounds=${AB_ROUNDS:-2}
samples=${AB_SAMPLES:-5}
case "$rounds" in '' | *[!0-9]*) echo "AB_ROUNDS must be a whole number" >&2; exit 2 ;; esac
case "$samples" in '' | *[!0-9]*) echo "AB_SAMPLES must be a whole number" >&2; exit 2 ;; esac
if [ "$rounds" -lt 1 ] || [ "$rounds" -gt 20 ]; then
  echo "AB_ROUNDS must be from 1 to 20" >&2
  exit 2
fi
for app in "$base_app" "$head_app"; do
  if [ ! -f "$app" ]; then
    echo "The app under test is missing: $app" >&2
    exit 2
  fi
done

run() {
  local side=$1 app=$2 round=$3
  shift 3
  local results="$out/$side/round-$round"
  local offset=$(((round - 1) * samples))
  mkdir -p "$results"
  echo "::group::Round $round, $side ($samples samples per benchmark from target $offset)"
  local command=(pnpm --filter @blocks2cpp/desktop exec vitest run
    --config e2e/bench/vitest.bench.config.ts --project bench "$@")
  if [ "${AB_XVFB:-0}" = "1" ]; then
    command=(xvfb-run --auto-servernum "${command[@]}")
  fi
  B2C_E2E_APP=$app B2C_BENCH_OUT=$results B2C_BENCH_SAMPLES=$samples B2C_BENCH_OFFSET=$offset \
    "${command[@]}"
  echo "::endgroup::"
}

for round in $(seq 1 "$rounds"); do
  if [ $((round % 2)) -eq 1 ]; then
    run base "$base_app" "$round" "$@"
    run head "$head_app" "$round" "$@"
  else
    run head "$head_app" "$round" "$@"
    run base "$base_app" "$round" "$@"
  fi
done
