#!/usr/bin/env python3
"""Runs a program and stops it when its resident memory passes a limit.

The cargo target runner of the weekly mutation job (.github/workflows/weekly.yml,
docs/spec/09-quality-and-delivery.md §9.2): with

    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="python3 <checkout>/tools/memory-capped-runner.py --limit-mib 2048 --"

cargo starts every test binary and doctest through it. Some mutants turn a
bounded loop into an endless one that allocates (`index += 1` becoming
`index *= 1`); uncapped, such a test fills the runner's memory within the
per-mutant timeout and the runner is shut down, taking the whole shard with it.
Capped, the test process is killed at the limit, `cargo test` fails, and
cargo-mutants counts the mutant as caught, which it is: the change does not go
unnoticed.

The cap is on each test process, never on the whole run (cargo, rustc and the
linker are not affected), and it is a cap on resident memory, polled from
/proc, rather than an address-space limit (`prlimit --as`, `ulimit -v`): a
resource limit is inherited by every compiler and program a test starts, and
the tests build and run AddressSanitizer programs, which reserve about 15 TiB
of address space for their shadow memory and cannot start under any such
limit. The toolchain probe would then report that AddressSanitizer does not
work, so the tests would see a different product rather than a capped one. The
processes a test starts (compilers, the programs it builds) are not counted
against its cap.

The runner passes standard input, output and error, the arguments and the
environment through unchanged, and forwards SIGTERM, SIGINT and SIGHUP to the
program. It exits with the program's exit status, with 128 + N when the program
was ended by signal N (as a shell reports it), with 137 (128 + SIGKILL) when it
stopped the program at the limit, with 126 or 127 when the program could not be
started, and with 2 on a usage error. It needs Linux (/proc/<pid>/statm).

Usage: memory-capped-runner.py [--limit-mib N] [--interval-ms N] -- PROGRAM [ARGUMENT...]
       memory-capped-runner.py --self-test
"""

from __future__ import annotations

import argparse
import contextlib
import os
import signal
import subprocess
import sys

NAME = "memory-capped-runner"

# The default cap. The largest test process of the mutation scope peaks at
# about 340 MiB resident (b2c-model's unit tests, measured 2026-10-15).
DEFAULT_LIMIT_MIB = 2048
# How often the program's resident memory is read. The endless allocation of
# the filter_pkg_config mutant grows by about 0.4 GiB/s and was stopped 11 MiB
# past a 2048 MiB limit.
DEFAULT_INTERVAL_MS = 50

FORWARDED_SIGNALS = (signal.SIGTERM, signal.SIGINT, signal.SIGHUP)


class UsageError(Exception):
    """A bad command line (exit status 2)."""


def parse(argv: list[str]) -> argparse.Namespace:
    """Splits the runner's options from the program at the first `--`."""
    parser = argparse.ArgumentParser(
        prog="memory-capped-runner.py",
        description=__doc__.split("\n\n", 1)[0],
        usage="%(prog)s [--limit-mib N] [--interval-ms N] -- PROGRAM [ARGUMENT...]\n"
        "       %(prog)s --self-test",
    )
    parser.add_argument(
        "--limit-mib",
        type=int,
        default=DEFAULT_LIMIT_MIB,
        help=f"the program's resident memory limit in MiB (default {DEFAULT_LIMIT_MIB})",
    )
    parser.add_argument(
        "--interval-ms",
        type=int,
        default=DEFAULT_INTERVAL_MS,
        help=f"how often the resident memory is read, in ms (default {DEFAULT_INTERVAL_MS})",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="check the runner itself with programs that exit, crash and allocate",
    )
    if "--" in argv:
        split = argv.index("--")
        options, command = argv[:split], argv[split + 1 :]
    else:
        options, command = argv, []
    arguments = parser.parse_args(options)
    arguments.command = command
    if arguments.self_test:
        if command:
            raise UsageError("--self-test takes no program")
        return arguments
    if not command:
        raise UsageError("no program given (put it after --)")
    if not 1 <= arguments.limit_mib <= 1 << 20:
        raise UsageError(f"--limit-mib must be from 1 to {1 << 20}, not {arguments.limit_mib}")
    if not 1 <= arguments.interval_ms <= 10_000:
        raise UsageError(f"--interval-ms must be from 1 to 10000, not {arguments.interval_ms}")
    return arguments


def resident_bytes(pid: int, page_size: int) -> int | None:
    """The resident memory of a process, or None once it is gone.

    Reads /proc/<pid>/statm, whose second field is the resident set in pages.
    A process that has exited but is not yet reaped reads as 0.
    """
    try:
        with open(f"/proc/{pid}/statm", encoding="ascii") as statm:
            return int(statm.read().split()[1]) * page_size
    except (FileNotFoundError, ProcessLookupError):
        return None


def exit_status(returncode: int) -> int:
    """A Popen return code as a shell reports it (128 + N for signal N)."""
    return returncode if returncode >= 0 else 128 - returncode


def run(command: list[str], limit_mib: int, interval_ms: int) -> int:
    """Runs the program until it exits or passes the limit; returns the exit status."""
    try:
        child = subprocess.Popen(command)
    except FileNotFoundError:
        print(f"{NAME}: {command[0]}: program not found", file=sys.stderr)
        return 127
    except PermissionError:
        print(f"{NAME}: {command[0]}: permission denied", file=sys.stderr)
        return 126

    def forward(signum: int, _frame: object) -> None:
        # Popen.send_signal does nothing once the program has been reaped.
        with contextlib.suppress(ProcessLookupError):
            child.send_signal(signum)

    for signum in FORWARDED_SIGNALS:
        signal.signal(signum, forward)

    limit = limit_mib << 20
    page_size = os.sysconf("SC_PAGE_SIZE")
    interval = interval_ms / 1000
    while True:
        try:
            return exit_status(child.wait(timeout=interval))
        except subprocess.TimeoutExpired:
            pass
        resident = resident_bytes(child.pid, page_size)
        if resident is not None and resident > limit:
            child.kill()
            child.wait()
            print(
                f"{NAME}: stopped {command[0]}: its resident memory reached "
                f"{resident >> 20} MiB, above the limit of {limit_mib} MiB "
                "(an endless allocation?)",
                file=sys.stderr,
                flush=True,
            )
            return 128 + signal.SIGKILL


# A program that allocates (and writes) the given number of MiB in 8 MiB
# steps, then exits with 0; the self-test runs it under a 64 MiB cap.
ALLOCATE = (
    "import sys, time\n"
    "chunks = []\n"
    "for _ in range(int(sys.argv[1]) // 8):\n"
    "    chunks.append(b'\\x01' * (8 << 20))\n"
    "    time.sleep(0.01)\n"
)


def self_test() -> int:
    """Runs this script on small Python programs and checks what it reports."""
    python = sys.executable
    script = os.path.abspath(__file__)
    failures: list[str] = []
    cases = 0

    def runner(
        *program: str, limit_mib: int = 64, stdin: str = ""
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [python, script, "--limit-mib", str(limit_mib), "--interval-ms", "20"]
            + (["--", *program] if program else []),
            input=stdin,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
        )

    def expect(what: str, condition: bool, result: subprocess.CompletedProcess[str]) -> None:
        nonlocal cases
        cases += 1
        if not condition:
            failures.append(
                f"{what}: exit status {result.returncode}, "
                f"stdout {result.stdout!r}, stderr {result.stderr[-500:]!r}"
            )

    result = runner(python, "-c", "import sys; sys.exit(3)")
    expect("the program's exit status is the runner's", result.returncode == 3, result)

    result = runner(
        python,
        "-c",
        "import sys; print(sys.argv[1:], repr(sys.stdin.read()))",
        "two words",
        "--nocapture",
        stdin="line\n",
    )
    expect(
        "arguments, standard input and output pass through unchanged",
        result.returncode == 0 and result.stdout == "['two words', '--nocapture'] 'line\\n'\n",
        result,
    )

    result = runner(python, "-c", "import os, signal; os.kill(os.getpid(), signal.SIGTERM)")
    expect(
        "a program ended by a signal gives 128 + its number",
        result.returncode == 128 + signal.SIGTERM,
        result,
    )

    result = runner(python, "-c", ALLOCATE, "16")
    expect(
        "a program that stays under the limit is not stopped",
        result.returncode == 0 and result.stderr == "",
        result,
    )

    # Without the cap the program would allocate 1 GiB and exit with 0.
    result = runner(python, "-c", ALLOCATE, "1024")
    expect(
        "a program that allocates past the limit is stopped with 137",
        result.returncode == 128 + signal.SIGKILL and "above the limit of 64 MiB" in result.stderr,
        result,
    )

    result = runner(os.path.join(os.path.dirname(script), "no-such-program"))
    expect("a missing program gives 127", result.returncode == 127, result)

    result = runner()
    expect("no program is a usage error", result.returncode == 2, result)
    result = runner(python, "-c", "pass", limit_mib=0)
    expect("a limit of 0 is a usage error", result.returncode == 2, result)

    if failures:
        print("The memory-capped runner self-test failed:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print(f"Memory-capped runner self-test passed ({cases} cases).")
    return 0


def main(argv: list[str]) -> int:
    try:
        arguments = parse(argv)
    except UsageError as error:
        print(f"{NAME}: {error}", file=sys.stderr)
        return 2
    if arguments.self_test:
        return self_test()
    if not os.path.exists(f"/proc/{os.getpid()}/statm"):
        print(f"{NAME}: needs Linux (/proc/<pid>/statm) to read resident memory", file=sys.stderr)
        return 2
    return run(arguments.command, arguments.limit_mib, arguments.interval_ms)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
