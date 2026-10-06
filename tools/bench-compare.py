#!/usr/bin/env python3
"""Compares a benchmark run with the baseline and fails on a regression.

The benchmark gate of docs/spec/09-quality-and-delivery.md §9.2: every metric
of a run has at least 10 samples; its median is compared with the baseline,
the median of the medians of the last 5 successful nightly runs on the
default branch for the same system. A gated metric more than 10% worse fails
the run. Until a metric has 5 baselines, its comparison is only reported.

Inputs and outputs are JSON files with a format tag:

* results (``blocks2cpp/bench-results``): what one run measured, one file per
  producer (the criterion conversion below, each webview benchmark):

      {"format": "blocks2cpp/bench-results", "formatVersion": 1,
       "os": "linux" | "windows",
       "metrics": [{"name": "webview.cold-start", "unit": "ms",
                    "better": "lower" | "higher", "gate": true,
                    "description": "...", "samples": [1234.5, ...]}]}

* history (``blocks2cpp/bench-history``): the medians of the last 5 runs that
  became baselines, kept between nightly runs in the Actions cache:

      {"format": "blocks2cpp/bench-history", "formatVersion": 1, "os": "linux",
       "runs": [{"run": "<run id>", "commit": "<sha>", "date": "<UTC time>",
                 "medians": {"<metric>": {"value": 1234.5, "unit": "ms"}}}]}

Commands (run from anywhere; paths are taken as given):

    bench-compare.py criterion --dir <target>/criterion --group pipeline \\
        --os linux --gate preview --out native.json
        Converts criterion's samples (<dir>/<group>/<bench>/new/sample.json)
        into a results file: metric "native.<group>.<bench>", one sample per
        criterion sample (its mean time per iteration), in milliseconds.

    bench-compare.py compare --os linux (--results a.json b.json | --results-dir DIR) \\
        [--history history.json] [--update-history history.json \\
         --run-id ID --commit SHA] [--summary summary.md]
        Compares the metrics of the results files (given, or every *.json in
        DIR) and prints a table (and appends it to --summary, such as
        $GITHUB_STEP_SUMMARY). With --update-history, a run without a
        regression is added to the history, which keeps the last 5 runs.

    bench-compare.py --self-test
        Checks this script against synthetic fixtures (equal, +9% and +11%
        medians, too few baselines, too few samples, malformed input, history
        updates, the criterion conversion).

Exit status: 0 when nothing regressed (or only informational comparisons
were made), 1 on a regression of a gated metric, 2 when the input is not
usable (the message says why).
"""

from __future__ import annotations

import argparse
import datetime as dt
import io
import json
import math
import re
import statistics
import sys
import tempfile
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import dataclass, field
from pathlib import Path

RESULTS_FORMAT = "blocks2cpp/bench-results"
HISTORY_FORMAT = "blocks2cpp/bench-history"
FORMAT_VERSION = 1

# The defaults of the gate (09 §9.2).
DEFAULT_THRESHOLD = 0.10
DEFAULT_BASELINE_RUNS = 5
DEFAULT_MIN_SAMPLES = 10

# Limits on what is read: these files come from earlier runs and from the
# benchmarks, so they are checked before they are trusted.
MAX_FILE_BYTES = 4 * 1024 * 1024
MAX_METRICS = 64
MAX_SAMPLES = 100_000
MAX_VALUE = 1e12
MAX_HISTORY_RUNS = 50
MAX_DESCRIPTION = 200
SYSTEMS = ("linux", "windows", "macos")
UNITS = ("ns", "us", "ms", "s")
DIRECTIONS = ("lower", "higher")
METRIC_NAME = re.compile(r"^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$")
MAX_METRIC_NAME = 64
BENCH_NAME = re.compile(r"^[a-z][a-z0-9_-]{0,31}$")
RUN_FIELD = re.compile(r"^[0-9A-Za-z._-]{1,64}$")
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")

RESULT_KEYS = {"format", "formatVersion", "os", "metrics"}
METRIC_KEYS = {"name", "unit", "better", "gate", "description", "samples"}
HISTORY_KEYS = {"format", "formatVersion", "os", "runs"}
RUN_KEYS = {"run", "commit", "date", "medians"}
MEDIAN_KEYS = {"value", "unit"}


class InputError(Exception):
    """A file or argument that cannot be used (exit status 2)."""


@dataclass(frozen=True)
class Metric:
    """One metric of a run."""

    name: str
    unit: str
    better: str
    gate: bool
    description: str
    samples: tuple[float, ...]

    @property
    def median(self) -> float:
        return statistics.median(self.samples)


@dataclass(frozen=True)
class Baseline:
    """One run's median of a metric, as the history keeps it."""

    value: float
    unit: str


@dataclass(frozen=True)
class HistoryRun:
    """One run that became a baseline."""

    run: str
    commit: str
    date: str
    medians: dict[str, Baseline]


@dataclass
class History:
    """The kept runs of one system, oldest first."""

    system: str
    runs: list[HistoryRun] = field(default_factory=list)


@dataclass(frozen=True)
class Comparison:
    """What the comparison of one metric found."""

    metric: Metric
    baselines: int
    baseline: float | None
    change: float | None
    status: str  # new, informational, ok, regression, worse-ungated

    @property
    def failed(self) -> bool:
        return self.status == "regression"


# --- Reading and checking ----------------------------------------------------


def read_json(path: Path, what: str) -> object:
    """Reads a JSON file of at most MAX_FILE_BYTES with no duplicate keys."""
    try:
        size = path.stat().st_size
    except OSError as error:
        raise InputError(f"{what} {path} cannot be read: {error.strerror}") from error
    if size > MAX_FILE_BYTES:
        raise InputError(f"{what} {path} is {size} bytes; at most {MAX_FILE_BYTES} are read")

    def no_duplicates(pairs: list[tuple[str, object]]) -> dict[str, object]:
        result: dict[str, object] = {}
        for key, value in pairs:
            if key in result:
                raise InputError(f"{what} {path} has the key {key!r} twice")
            result[key] = value
        return result

    def no_constants(name: str) -> object:
        raise InputError(f"{what} {path} contains {name}, which is not JSON")

    try:
        text = path.read_text(encoding="utf-8")
        return json.loads(text, object_pairs_hook=no_duplicates, parse_constant=no_constants)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise InputError(f"{what} {path} is not valid JSON: {error}") from error


def exact_keys(value: object, keys: set[str], where: str) -> dict:
    if not isinstance(value, dict):
        raise InputError(f"{where} must be an object")
    missing = keys - value.keys()
    unknown = value.keys() - keys
    if missing:
        raise InputError(f"{where} is missing {', '.join(sorted(missing))}")
    if unknown:
        raise InputError(f"{where} has unknown keys: {', '.join(sorted(unknown))}")
    return value


def format_header(value: dict, expected: str, where: str) -> None:
    if value.get("format") != expected:
        raise InputError(f"{where} is not a {expected} file")
    version = value.get("formatVersion")
    if not isinstance(version, int) or isinstance(version, bool) or version != FORMAT_VERSION:
        raise InputError(f"{where} has formatVersion {version!r}; this script reads {FORMAT_VERSION}")


def system_name(value: object, where: str) -> str:
    if value not in SYSTEMS:
        raise InputError(f"{where}: os must be one of {', '.join(SYSTEMS)}")
    return str(value)


def number(value: object, where: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise InputError(f"{where} must be a number")
    result = float(value)
    if not math.isfinite(result) or result < 0 or result > MAX_VALUE:
        raise InputError(f"{where} must be a finite number from 0 to {MAX_VALUE:g}")
    return result


def metric_name(value: object, where: str) -> str:
    if not isinstance(value, str) or len(value) > MAX_METRIC_NAME or not METRIC_NAME.match(value):
        raise InputError(f"{where}: {value!r} is not a metric name (lower-case words joined by . or -)")
    return value


def unit_name(value: object, where: str) -> str:
    if value not in UNITS:
        raise InputError(f"{where}: unit must be one of {', '.join(UNITS)}")
    return str(value)


def parse_metric(value: object, where: str) -> Metric:
    item = exact_keys(value, METRIC_KEYS, where)
    name = metric_name(item["name"], where)
    where = f"{where} ({name})"
    better = item["better"]
    if better not in DIRECTIONS:
        raise InputError(f"{where}: better must be one of {', '.join(DIRECTIONS)}")
    if not isinstance(item["gate"], bool):
        raise InputError(f"{where}: gate must be true or false")
    description = item["description"]
    if (
        not isinstance(description, str)
        or len(description) > MAX_DESCRIPTION
        or any(ord(c) < 0x20 for c in description)
    ):
        raise InputError(f"{where}: description must be one line of at most {MAX_DESCRIPTION} characters")
    samples = item["samples"]
    if not isinstance(samples, list) or not 1 <= len(samples) <= MAX_SAMPLES:
        raise InputError(f"{where}: samples must be a list of 1 to {MAX_SAMPLES} numbers")
    return Metric(
        name=name,
        unit=unit_name(item["unit"], where),
        better=str(better),
        gate=item["gate"],
        description=description,
        samples=tuple(number(sample, f"{where} sample {index}") for index, sample in enumerate(samples)),
    )


def read_results(path: Path) -> tuple[str, list[Metric]]:
    """A results file's system and metrics."""
    where = f"results file {path}"
    value = exact_keys(read_json(path, "The results file"), RESULT_KEYS, where)
    format_header(value, RESULTS_FORMAT, where)
    system = system_name(value["os"], where)
    metrics = value["metrics"]
    if not isinstance(metrics, list) or not 1 <= len(metrics) <= MAX_METRICS:
        raise InputError(f"{where}: metrics must be a list of 1 to {MAX_METRICS} metrics")
    parsed = [parse_metric(item, f"{where}, metric {index}") for index, item in enumerate(metrics)]
    return system, parsed


def read_history(path: Path | None, system: str) -> History:
    """The kept runs, or an empty history when there is no file yet."""
    if path is None or not path.exists():
        return History(system)
    where = f"history file {path}"
    value = exact_keys(read_json(path, "The history file"), HISTORY_KEYS, where)
    format_header(value, HISTORY_FORMAT, where)
    if system_name(value["os"], where) != system:
        raise InputError(f"{where} is for {value['os']}, not {system}")
    runs = value["runs"]
    if not isinstance(runs, list) or len(runs) > MAX_HISTORY_RUNS:
        raise InputError(f"{where}: runs must be a list of at most {MAX_HISTORY_RUNS} runs")
    history = History(system)
    for index, item in enumerate(runs):
        run_where = f"{where}, run {index}"
        run = exact_keys(item, RUN_KEYS, run_where)
        for key in ("run", "commit"):
            if not isinstance(run[key], str) or not RUN_FIELD.match(run[key]):
                raise InputError(f"{run_where}: {key} is not a plain identifier")
        if not isinstance(run["date"], str) or not DATE.match(run["date"]):
            raise InputError(f"{run_where}: date must be a UTC time like 2026-10-06T03:23:00Z")
        medians = run["medians"]
        if not isinstance(medians, dict) or len(medians) > MAX_METRICS:
            raise InputError(f"{run_where}: medians must be an object of at most {MAX_METRICS} metrics")
        parsed: dict[str, Baseline] = {}
        for name, entry in medians.items():
            metric_name(name, run_where)
            median = exact_keys(entry, MEDIAN_KEYS, f"{run_where}, median {name}")
            parsed[name] = Baseline(
                value=number(median["value"], f"{run_where}, median {name}"),
                unit=unit_name(median["unit"], f"{run_where}, median {name}"),
            )
        history.runs.append(HistoryRun(run["run"], run["commit"], run["date"], parsed))
    return history


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def history_json(history: History) -> dict:
    return {
        "format": HISTORY_FORMAT,
        "formatVersion": FORMAT_VERSION,
        "os": history.system,
        "runs": [
            {
                "run": run.run,
                "commit": run.commit,
                "date": run.date,
                "medians": {
                    name: {"value": median.value, "unit": median.unit}
                    for name, median in sorted(run.medians.items())
                },
            }
            for run in history.runs
        ],
    }


# --- Comparing ---------------------------------------------------------------


def compare_metric(
    metric: Metric, history: History, *, threshold: float, baseline_runs: int
) -> Comparison:
    """Compares one metric with the median of its last `baseline_runs` baselines."""
    values = [
        run.medians[metric.name].value
        for run in history.runs
        if metric.name in run.medians and run.medians[metric.name].unit == metric.unit
    ][-baseline_runs:]
    if not values:
        return Comparison(metric, 0, None, None, "new")
    baseline = statistics.median(values)
    current = metric.median
    if baseline == 0:
        change = 0.0 if current == 0 else math.inf
    else:
        change = current / baseline - 1
    worse = change > threshold if metric.better == "lower" else change < -threshold
    if len(values) < baseline_runs:
        status = "informational"
    elif not worse:
        status = "ok"
    elif metric.gate:
        status = "regression"
    else:
        status = "worse-ungated"
    return Comparison(metric, len(values), baseline, change, status)


STATUS_TEXT = {
    "new": "new: no baseline yet",
    "informational": "informational: fewer than {runs} baselines",
    "ok": "ok",
    "regression": "**regression**",
    "worse-ungated": "worse (not gated)",
}


def fmt(value: float) -> str:
    if value >= 100:
        return f"{value:.1f}"
    if value >= 1:
        return f"{value:.2f}"
    return f"{value:.4f}"


def summary_markdown(
    system: str, comparisons: list[Comparison], *, threshold: float, baseline_runs: int
) -> str:
    lines = [
        f"### Benchmarks ({system})",
        "",
        f"Median of each metric's samples against the median of the last {baseline_runs} nightly "
        f"runs on the default branch; a gated metric more than {threshold:.0%} worse fails "
        "(docs/spec/09-quality-and-delivery.md §9.2).",
        "",
        "| Metric | Gated | Samples | Median | Baseline (runs) | Change | Result |",
        "| --- | --- | ---: | ---: | ---: | ---: | --- |",
    ]
    for comparison in comparisons:
        metric = comparison.metric
        baseline = (
            "—"
            if comparison.baseline is None
            else f"{fmt(comparison.baseline)} {metric.unit} ({comparison.baselines})"
        )
        change = "—" if comparison.change is None else f"{comparison.change:+.1%}"
        status = STATUS_TEXT[comparison.status].format(runs=baseline_runs)
        lines.append(
            f"| `{metric.name}` | {'yes' if metric.gate else 'no'} | {len(metric.samples)} | "
            f"{fmt(metric.median)} {metric.unit} | {baseline} | {change} | {status} |"
        )
    regressions = [c.metric.name for c in comparisons if c.failed]
    lines.append("")
    if regressions:
        lines.append(f"Regressed: {', '.join(f'`{name}`' for name in regressions)}.")
    else:
        lines.append("No gated metric regressed.")
    return "\n".join(lines) + "\n"


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def command_compare(arguments: argparse.Namespace) -> int:
    system = system_name(arguments.os, "--os")
    if not 0 < arguments.threshold < 1:
        raise InputError("--threshold must be between 0 and 1")
    if not 1 <= arguments.baseline_runs <= MAX_HISTORY_RUNS:
        raise InputError(f"--baseline-runs must be from 1 to {MAX_HISTORY_RUNS}")
    if arguments.min_samples < 1:
        raise InputError("--min-samples must be at least 1")
    if arguments.update_history is not None and (arguments.run_id is None or arguments.commit is None):
        raise InputError("--update-history needs --run-id and --commit")
    for key in ("run_id", "commit"):
        value = getattr(arguments, key)
        if value is not None and not RUN_FIELD.match(value):
            raise InputError(f"--{key.replace('_', '-')} is not a plain identifier")

    paths = [Path(path) for path in arguments.results or []]
    if arguments.results_dir is not None:
        folder = Path(arguments.results_dir)
        if not folder.is_dir():
            raise InputError(f"--results-dir {folder} is not a folder")
        found = sorted(path for path in folder.glob("*.json") if path.is_file())
        if not found:
            raise InputError(f"--results-dir {folder} has no results files (*.json)")
        paths.extend(found)
    if not paths:
        raise InputError("give the results files with --results or --results-dir")
    if len(paths) > MAX_METRICS:
        raise InputError(f"{len(paths)} results files; at most {MAX_METRICS} are read")

    metrics: dict[str, Metric] = {}
    for path in paths:
        results_system, parsed = read_results(path)
        if results_system != system:
            raise InputError(f"results file {path} is for {results_system}, not {system}")
        for metric in parsed:
            if metric.name in metrics:
                raise InputError(f"metric {metric.name} is in more than one results file")
            if len(metric.samples) < arguments.min_samples:
                raise InputError(
                    f"metric {metric.name} has {len(metric.samples)} samples; "
                    f"at least {arguments.min_samples} are needed"
                )
            metrics[metric.name] = metric
    if len(metrics) > MAX_METRICS:
        raise InputError(f"the results have {len(metrics)} metrics; at most {MAX_METRICS}")

    history_path = None if arguments.history is None else Path(arguments.history)
    history = read_history(history_path, system)
    comparisons = [
        compare_metric(
            metrics[name], history, threshold=arguments.threshold, baseline_runs=arguments.baseline_runs
        )
        for name in sorted(metrics)
    ]
    text = summary_markdown(
        system, comparisons, threshold=arguments.threshold, baseline_runs=arguments.baseline_runs
    )
    print(text, end="")
    if arguments.summary is not None:
        with Path(arguments.summary).open("a", encoding="utf-8") as summary:
            summary.write(text)

    failed = any(comparison.failed for comparison in comparisons)
    if arguments.update_history is not None:
        if failed:
            print("The run regressed, so it is not added to the history.")
        else:
            history.runs.append(
                HistoryRun(
                    run=arguments.run_id,
                    commit=arguments.commit,
                    date=arguments.date or utc_now(),
                    medians={
                        name: Baseline(value=metric.median, unit=metric.unit)
                        for name, metric in metrics.items()
                    },
                )
            )
            history.runs = history.runs[-arguments.baseline_runs :]
            write_json(Path(arguments.update_history), history_json(history))
            count = len(history.runs)
            print(f"The history now has {count} run{'' if count == 1 else 's'}.")
    return 1 if failed else 0


# --- criterion ---------------------------------------------------------------


def criterion_samples(path: Path) -> tuple[float, ...]:
    """The mean time per iteration of each criterion sample, in milliseconds."""
    value = read_json(path, "The criterion sample file")
    if not isinstance(value, dict):
        raise InputError(f"criterion sample file {path} must be an object")
    iterations = value.get("iters")
    times = value.get("times")
    if (
        not isinstance(iterations, list)
        or not isinstance(times, list)
        or len(iterations) != len(times)
        or not 1 <= len(times) <= MAX_SAMPLES
    ):
        raise InputError(f"criterion sample file {path} must have as many iters as times")
    samples = []
    for index, (count, total) in enumerate(zip(iterations, times, strict=True)):
        count = number(count, f"{path} iters {index}")
        total = number(total, f"{path} times {index}")
        if count < 1:
            raise InputError(f"{path} iters {index} must be at least 1")
        samples.append(total / count / 1e6)
    return tuple(samples)


def command_criterion(arguments: argparse.Namespace) -> int:
    system = system_name(arguments.os, "--os")
    if not BENCH_NAME.match(arguments.group):
        raise InputError(f"--group {arguments.group!r} is not a plain benchmark name")
    for gated in arguments.gate:
        if not BENCH_NAME.match(gated):
            raise InputError(f"--gate {gated!r} is not a plain benchmark name")
    group = Path(arguments.dir) / arguments.group
    if not group.is_dir():
        raise InputError(f"criterion found no group {arguments.group} in {arguments.dir}")
    metrics = []
    for bench in sorted(group.iterdir()):
        sample = bench / "new" / "sample.json"
        if not bench.is_dir() or bench.name == "report" or not sample.is_file():
            continue
        if not BENCH_NAME.match(bench.name):
            raise InputError(f"criterion benchmark {bench.name!r} is not a plain benchmark name")
        metrics.append(
            {
                "name": f"native.{arguments.group}.{bench.name}",
                "unit": "ms",
                "better": "lower",
                "gate": bench.name in arguments.gate,
                "description": f"criterion {arguments.group}/{bench.name}: mean time per iteration of each sample",
                "samples": list(criterion_samples(sample)),
            }
        )
    if not metrics:
        raise InputError(f"criterion group {group} has no benchmark results")
    if len(metrics) > MAX_METRICS:
        raise InputError(f"criterion group {group} has more than {MAX_METRICS} benchmarks")
    missing = set(arguments.gate) - {metric["name"].rsplit(".", 1)[-1] for metric in metrics}
    if missing:
        raise InputError(f"no results for the gated benchmarks {', '.join(sorted(missing))}")
    write_json(
        Path(arguments.out),
        {"format": RESULTS_FORMAT, "formatVersion": FORMAT_VERSION, "os": system, "metrics": metrics},
    )
    print(f"Wrote {len(metrics)} criterion metrics to {arguments.out}.")
    return 0


# --- Command line ------------------------------------------------------------


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(
        prog="bench-compare.py",
        description="Compares a benchmark run with the baseline (docs/spec/09-quality-and-delivery.md §9.2).",
    )
    root.add_argument("--self-test", action="store_true", help="check this script on synthetic fixtures")
    commands = root.add_subparsers(dest="command")

    criterion = commands.add_parser("criterion", help="convert criterion's samples into a results file")
    criterion.add_argument("--dir", required=True, help="criterion's output folder (<target>/criterion)")
    criterion.add_argument("--group", required=True, help="the benchmark group")
    criterion.add_argument("--os", required=True, help="linux, windows or macos")
    criterion.add_argument("--gate", action="append", default=[], help="a benchmark to gate (repeatable)")
    criterion.add_argument("--out", required=True, help="the results file to write")

    compare = commands.add_parser("compare", help="compare results with the history")
    compare.add_argument("--os", required=True, help="linux, windows or macos")
    compare.add_argument("--results", nargs="+", help="results files")
    compare.add_argument("--results-dir", help="a folder whose *.json files are all results files")
    compare.add_argument("--history", help="the history file (missing: no baselines yet)")
    compare.add_argument("--update-history", help="write the history with this run added (no regression only)")
    compare.add_argument("--run-id", help="this run's ID, for the history")
    compare.add_argument("--commit", help="this run's commit, for the history")
    compare.add_argument("--date", help="this run's UTC time (default: now)")
    compare.add_argument("--summary", help="append the Markdown table to this file")
    compare.add_argument("--threshold", type=float, default=DEFAULT_THRESHOLD)
    compare.add_argument("--baseline-runs", type=int, default=DEFAULT_BASELINE_RUNS)
    compare.add_argument("--min-samples", type=int, default=DEFAULT_MIN_SAMPLES)
    return root


def main(argv: list[str]) -> int:
    root = parser()
    arguments = root.parse_args(argv)
    if arguments.self_test:
        return self_test()
    try:
        if arguments.command == "criterion":
            return command_criterion(arguments)
        if arguments.command == "compare":
            if arguments.date is not None and not DATE.match(arguments.date):
                raise InputError("--date must be a UTC time like 2026-10-06T03:23:00Z")
            return command_compare(arguments)
    except InputError as error:
        print(f"bench-compare: {error}", file=sys.stderr)
        return 2
    root.print_usage(sys.stderr)
    return 2


# --- Self-test ---------------------------------------------------------------


def _results(path: Path, system: str, metrics: list[dict]) -> Path:
    write_json(
        path, {"format": RESULTS_FORMAT, "formatVersion": FORMAT_VERSION, "os": system, "metrics": metrics}
    )
    return path


def _metric(
    name: str,
    median: float,
    *,
    samples: int = 10,
    gate: bool = True,
    better: str = "lower",
    unit: str = "ms",
) -> dict:
    # Symmetric around the median, so the median is exact.
    values = [median * (1 + (index - (samples - 1) / 2) * 0.001) for index in range(samples)]
    return {
        "name": name,
        "unit": unit,
        "better": better,
        "gate": gate,
        "description": "synthetic",
        "samples": values,
    }


def _history(path: Path, system: str, medians: list[dict[str, float]], unit: str = "ms") -> Path:
    history = History(system)
    for index, values in enumerate(medians):
        history.runs.append(
            HistoryRun(
                run=str(100 + index),
                commit=f"c{index:039d}",
                date=f"2026-10-0{index + 1}T03:23:00Z",
                medians={name: Baseline(value, unit) for name, value in values.items()},
            )
        )
    write_json(path, history_json(history))
    return path


def _run(argv: list[str]) -> tuple[int, str]:
    out = io.StringIO()
    with redirect_stdout(out), redirect_stderr(out):
        status = main(argv)
    return status, out.getvalue()


def self_test() -> int:
    failures: list[str] = []

    def expect(name: str, argv: list[str], status: int, *texts: str) -> str:
        actual, output = _run(argv)
        if actual != status:
            failures.append(f"{name}: exit status {actual}, expected {status}\n{output}")
        for text in texts:
            if text not in output:
                failures.append(f"{name}: the output does not contain {text!r}\n{output}")
        return output

    with tempfile.TemporaryDirectory(prefix="bench-compare-") as folder:
        root = Path(folder)
        five = _history(root / "five.json", "linux", [{"m.time": 100.0}] * 5)
        four = _history(root / "four.json", "linux", [{"m.time": 100.0}] * 4)
        # The baseline is the median of the last five runs, not of all of them.
        six = _history(
            root / "six.json",
            "linux",
            [{"m.time": 500.0}] + [{"m.time": float(v)} for v in (90, 95, 100, 105, 110)],
        )

        def compare(results: Path, history: Path, *extra: str) -> list[str]:
            return ["compare", "--os", "linux", "--results", str(results), "--history", str(history), *extra]

        equal = _results(root / "equal.json", "linux", [_metric("m.time", 100.0)])
        expect("equal medians pass", compare(equal, five), 0, "| ok |")
        plus9 = _results(root / "plus9.json", "linux", [_metric("m.time", 109.0)])
        expect("+9% passes", compare(plus9, five), 0, "+9.0%", "| ok |")
        plus11 = _results(root / "plus11.json", "linux", [_metric("m.time", 111.0)])
        expect("+11% fails", compare(plus11, five), 1, "+11.0%", "**regression**", "Regressed: `m.time`")
        expect("+11% on the last five fails", compare(plus11, six), 1, "100.0 ms (5)")
        expect("+11% with four baselines is informational", compare(plus11, four), 0, "informational")
        expect("no history file is new", compare(plus11, root / "missing.json"), 0, "new: no baseline yet")
        faster = _results(root / "faster.json", "linux", [_metric("m.time", 50.0)])
        expect("faster passes", compare(faster, five), 0, "-50.0%", "| ok |")
        ungated = _results(root / "ungated.json", "linux", [_metric("m.time", 150.0, gate=False)])
        expect("an ungated metric only reports", compare(ungated, five), 0, "worse (not gated)")
        higher = _results(root / "higher.json", "linux", [_metric("m.time", 89.0, better="higher")])
        expect("higher-is-better regresses when lower", compare(higher, five), 1, "**regression**")
        seconds = _results(root / "seconds.json", "linux", [_metric("m.time", 0.2, unit="s")])
        expect("a changed unit has no baseline", compare(seconds, five), 0, "new: no baseline yet")

        few = _results(root / "few.json", "linux", [_metric("m.time", 100.0, samples=9)])
        expect("fewer than 10 samples are refused", compare(few, five), 2, "at least 10 are needed")
        expect(
            "another system's results are refused",
            ["compare", "--os", "windows", "--results", str(equal)],
            2,
            "is for linux, not windows",
        )
        expect(
            "another system's history is refused",
            ["compare", "--os", "windows", "--results", str(_results(root / "w.json", "windows", [_metric("m.time", 1.0)])),
             "--history", str(five)],
            2,
            "is for linux, not windows",
        )
        broken = root / "broken.json"
        broken.write_text('{"format": "blocks2cpp/bench-results", "format": 1}', encoding="utf-8")
        expect("duplicate keys are refused", compare(broken, five), 2, "twice")
        nan = root / "nan.json"
        nan.write_text(
            '{"format": "blocks2cpp/bench-results", "formatVersion": 1, "os": "linux", "metrics": '
            '[{"name": "m.time", "unit": "ms", "better": "lower", "gate": true, "description": "",'
            ' "samples": [1, 2, 3, 4, 5, 6, 7, 8, 9, NaN]}]}',
            encoding="utf-8",
        )
        expect("NaN is refused", compare(nan, five), 2, "contains NaN, which is not JSON")
        negative = _results(root / "negative.json", "linux", [{**_metric("m.time", 1.0), "samples": [-1.0] * 10}])
        expect("negative samples are refused", compare(negative, five), 2, "finite number from 0")
        unknown = _results(root / "unknown.json", "linux", [{**_metric("m.time", 1.0), "extra": 1}])
        expect("unknown keys are refused", compare(unknown, five), 2, "unknown keys: extra")
        bad_name = _results(root / "bad-name.json", "linux", [_metric("M Time", 1.0)])
        expect("bad metric names are refused", compare(bad_name, five), 2, "not a metric name")
        twice = [str(equal), str(plus9)]
        expect(
            "a metric in two files is refused",
            ["compare", "--os", "linux", "--results", *twice, "--history", str(five)],
            2,
            "more than one results file",
        )
        folder_of_results = root / "results"
        _results(folder_of_results / "a.json", "linux", [_metric("m.time", 100.0)])
        _results(folder_of_results / "b.json", "linux", [_metric("m.other", 5.0, gate=False)])
        (folder_of_results / "notes.txt").write_text("not a results file", encoding="utf-8")
        expect(
            "a folder of results files is read",
            ["compare", "--os", "linux", "--results-dir", str(folder_of_results), "--history", str(five)],
            0,
            "`m.other`",
            "| ok |",
        )
        empty = root / "empty"
        empty.mkdir()
        expect(
            "an empty results folder is refused",
            ["compare", "--os", "linux", "--results-dir", str(empty)],
            2,
            "has no results files",
        )
        expect("results are required", ["compare", "--os", "linux"], 2, "--results or --results-dir")

        # History updates: a passing run is added and the oldest dropped; a regression is not added.
        updated = root / "updated.json"
        expect(
            "a passing run updates the history",
            compare(
                plus9, five, "--update-history", str(updated),
                "--run-id", "999", "--commit", "abc", "--date", "2026-10-06T03:23:00Z",
            ),
            0,
            "history now has 5 runs",
        )
        kept = read_history(updated, "linux")
        if [run.run for run in kept.runs] != ["101", "102", "103", "104", "999"]:
            failures.append(f"history update: runs {[run.run for run in kept.runs]}")
        elif abs(kept.runs[-1].medians["m.time"].value - 109.0) > 1e-9:
            failures.append("history update: the median of the run was not kept")
        regressed = root / "regressed.json"
        expect(
            "a regression does not update the history",
            compare(plus11, five, "--update-history", str(regressed), "--run-id", "1", "--commit", "abc"),
            1,
            "not added to the history",
        )
        if regressed.exists():
            failures.append("a regressed run was written to the history")
        summary = root / "summary.md"
        expect("the summary is appended", compare(equal, five, "--summary", str(summary)), 0)
        if "### Benchmarks (linux)" not in summary.read_text(encoding="utf-8"):
            failures.append("the summary file was not written")

        # The criterion conversion.
        sample = root / "criterion" / "pipeline" / "preview" / "new" / "sample.json"
        write_json(sample, {"sampling_mode": "Flat", "iters": [4.0] * 10, "times": [80e6] * 10})
        write_json(
            root / "criterion" / "pipeline" / "load" / "new" / "sample.json",
            {"sampling_mode": "Flat", "iters": [2.0] * 10, "times": [10e6] * 10},
        )
        native = root / "native.json"
        convert = ["criterion", "--dir", str(root / "criterion"), "--group", "pipeline", "--os", "linux"]
        expect(
            "criterion samples convert",
            [*convert, "--gate", "preview", "--out", str(native)],
            0,
            "Wrote 2 criterion metrics",
        )
        _, converted = read_results(native)
        by_name = {metric.name: metric for metric in converted}
        preview = by_name.get("native.pipeline.preview")
        load = by_name.get("native.pipeline.load")
        if preview is None or load is None or not preview.gate or load.gate or abs(preview.median - 20.0) > 1e-9:
            failures.append(f"criterion conversion: {converted}")
        expect(
            "a gated benchmark without results is refused",
            [*convert, "--gate", "analyze", "--out", str(native)],
            2,
            "no results for the gated benchmarks analyze",
        )
        expect(
            "a missing group is refused",
            ["criterion", "--dir", str(root), "--group", "nothing", "--os", "linux", "--out", str(native)],
            2,
            "found no group nothing",
        )

    if failures:
        print("The bench-compare self-test failed:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("bench-compare self-test passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
