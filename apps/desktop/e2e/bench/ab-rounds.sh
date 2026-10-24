#!/usr/bin/env bash
# The A/B benchmark runs (docs/spec/09-quality-and-delivery.md §9.2, README.md "A/B runs"): the
# webview benchmarks of this checkout run against two builds of the app on one machine, in
# alternating runs, so that both builds meet the same machine state. Each benchmark runs in a
# Vitest call of its own, which writes its results files into
# <out>/<base|head>/round-<n>/<benchmark>/; tools/bench-compare.py ab adds up each side's samples
# and compares the medians (the edits' p95-all: the p95 of each side's pooled edits).
#
# Usage: ab-rounds.sh <base app> <head app> <out folder> <benchmark>...
#   <benchmark> is a benchmark file's name without ".e2e.ts" (preview.bench, select.bench, ...);
#   the out folder must be new or empty. For example:
#   ab-rounds.sh /tmp/base/blocks2cpp-desktop /tmp/head/blocks2cpp-desktop /tmp/ab \
#     preview.bench select.bench
#
# A benchmark that cannot be compared does not stop the others:
# - one the base build cannot run (its end-to-end hook lacks a method the benchmark calls,
#   bench/capabilities.ts) is skipped on both sides, with the reason: the base's runs set
#   B2C_BENCH_SKIP_NOTE, and a benchmark that skips writes why into that file;
# - one that fails (its Vitest call exits with an error) is not started again, except that the
#   head still runs it in the round where the base failed, so a failure of the head's build is
#   found too.
# Their results files move to <out>/left-out/, so that only benchmarks that ran in every round on
# both sides are compared, and <out>/ab-runs.tsv records what happened to each benchmark for
# `bench-compare.py ab --runs`, which fails on a gated regression or on a benchmark that failed on
# the head. The rounds' outcome is that file's: this script exits 0 once the rounds are done, and
# 2 when its arguments or settings cannot be used.
#
# ab-runs.tsv: the line "blocks2cpp/ab-runs<TAB>1", then one line per benchmark, in the order
# given: <benchmark> <TAB> compared|skipped|failed <TAB> <round the base failed in, or 0> <TAB>
# <round the head failed in, or 0> <TAB> <why it was skipped, or nothing>.
#
# Environment:
#   AB_ROUNDS   rounds (default 2); round n runs the base first when n is odd, the head first
#               otherwise (base, head, head, base, ...)
#   AB_SAMPLES  samples per benchmark and run (default 5; B2C_BENCH_SAMPLES of the runs). Round n
#               starts the benchmarks that go round a list of targets (the selection ones) at
#               (n - 1) * AB_SAMPLES (B2C_BENCH_OFFSET), the same for both builds, so the rounds
#               together click the targets a nightly run does
#   AB_XVFB     1: run each benchmark run under xvfb-run (Linux CI)
#   B2C_E2E_ARTIFACTS  when set, each run's artifacts go into <it>/<side>/round-<n>/<benchmark>/
# The rest of the harness's settings (e2e/README.md) are passed through; B2C_BENCH_SKIP_NOTE is
# this script's to set.
set -uo pipefail
unset B2C_BENCH_SKIP_NOTE

if [ "$#" -lt 4 ]; then
  echo "usage: $0 <base app> <head app> <out folder> <benchmark>..." >&2
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
benchmarks=("$@")

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
# A benchmark's name is a folder name and a field of ab-runs.tsv.
count=${#benchmarks[@]}
for ((i = 0; i < count; i++)); do
  if ! [[ "${benchmarks[i]}" =~ ^[a-z][a-z0-9-]{0,31}\.bench$ ]]; then
    echo "Not a benchmark name: '${benchmarks[i]}' (a file's name without .e2e.ts, such as preview.bench)" >&2
    exit 2
  fi
  for ((j = 0; j < i; j++)); do
    if [ "${benchmarks[i]}" = "${benchmarks[j]}" ]; then
      echo "The benchmark ${benchmarks[i]} is named twice" >&2
      exit 2
    fi
  done
done
# Results left over from another run would be compared with this one's.
if [ -e "$out" ] && { [ ! -d "$out" ] || [ -n "$(ls -A "$out")" ]; }; then
  echo "The out folder must be new or empty: $out" >&2
  exit 2
fi
mkdir -p "$out/base" "$out/head" "$out/notes"
artifacts=${B2C_E2E_ARTIFACTS:-}

# What happened to each benchmark so far (by its index in `benchmarks`): its outcome (running,
# skipped or failed), the rounds its base and head runs failed in (0: none) and why it was
# skipped.
outcome=()
base_failed=()
head_failed=()
reason=()
for ((i = 0; i < count; i++)); do
  outcome[i]=running
  base_failed[i]=0
  head_failed[i]=0
  reason[i]=""
done

# Whether benchmark $1 runs on side $2 in round $3: while it runs everywhere, and on the head in
# the round where the base failed it (the head may not have run it yet in that round).
due() {
  local i=$1 side=$2 round=$3
  case "${outcome[i]}" in
    running) return 0 ;;
    failed) [ "$side" = head ] && [ "${base_failed[i]}" -eq "$round" ] && [ "${head_failed[i]}" -eq 0 ] ;;
    *) return 1 ;;
  esac
}

# Runs benchmark $1 against side $2's app ($3) in round $4, and notes a skip or a failure.
run() {
  local i=$1 side=$2 app=$3 round=$4
  local bench=${benchmarks[i]}
  local results="$out/$side/round-$round/$bench"
  local note="$out/notes/$side-round-$round-$bench.txt"
  local offset=$(((round - 1) * samples))
  mkdir -p "$results"
  echo "::group::Round $round, $side: $bench ($samples samples from target $offset)"
  local command=(pnpm --filter @blocks2cpp/desktop exec vitest run
    --config e2e/bench/vitest.bench.config.ts --project bench "$bench")
  if [ "${AB_XVFB:-0}" = "1" ]; then
    command=(xvfb-run --auto-servernum "${command[@]}")
  fi
  local status=0
  (
    export B2C_E2E_APP="$app" B2C_BENCH_OUT="$results" B2C_BENCH_SAMPLES="$samples"
    export B2C_BENCH_OFFSET="$offset"
    # Only the base may lack a hook method: the head's build comes with these benchmarks.
    if [ "$side" = base ]; then
      export B2C_BENCH_SKIP_NOTE="$note"
    fi
    if [ -n "$artifacts" ]; then
      export B2C_E2E_ARTIFACTS="$artifacts/$side/round-$round/$bench"
    fi
    "${command[@]}"
  ) || status=$?
  echo "::endgroup::"
  if [ "$side" = base ] && [ -s "$note" ]; then
    outcome[i]=skipped
    # One line of printable ASCII, as ab-runs.tsv holds it.
    reason[i]=$(head -n 1 "$note" | LC_ALL=C tr -cd ' -~' | cut -c 1-300)
    echo "::notice::$bench is skipped: ${reason[i]}"
  elif [ "$status" -ne 0 ]; then
    outcome[i]=failed
    if [ "$side" = base ]; then
      base_failed[i]=$round
      echo "::warning::$bench failed on the base build in round $round (exit status $status): it is not compared"
    else
      head_failed[i]=$round
      echo "::error::$bench failed on the head build in round $round (exit status $status)"
    fi
  fi
}

for ((round = 1; round <= rounds; round++)); do
  if [ $((round % 2)) -eq 1 ]; then
    order=(base head)
  else
    order=(head base)
  fi
  for side in "${order[@]}"; do
    app=$base_app
    if [ "$side" = head ]; then
      app=$head_app
    fi
    for ((i = 0; i < count; i++)); do
      if due "$i" "$side" "$round"; then
        run "$i" "$side" "$app" "$round"
      fi
    done
  done
done

# Only what ran in every round on both sides is compared; the rest moves to left-out/.
summary=""
{
  printf 'blocks2cpp/ab-runs\t1\n'
  for ((i = 0; i < count; i++)); do
    bench=${benchmarks[i]}
    if [ "${outcome[i]}" = running ]; then
      outcome[i]=compared
    else
      # By name, not by a pattern: the out folder may be a Windows path with backslashes.
      for ((round = 1; round <= rounds; round++)); do
        for side in base head; do
          if [ -d "$out/$side/round-$round/$bench" ]; then
            mkdir -p "$out/left-out/$side/round-$round"
            mv "$out/$side/round-$round/$bench" "$out/left-out/$side/round-$round/$bench"
          fi
        done
      done
    fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$bench" "${outcome[i]}" "${base_failed[i]}" \
      "${head_failed[i]}" "${reason[i]}"
    summary+="${summary:+, }$bench ${outcome[i]}"
  done
} > "$out/ab-runs.tsv"
echo "A/B rounds done ($rounds of $samples samples per side): $summary (bench-compare.py ab --runs $out/ab-runs.tsv judges them)"
