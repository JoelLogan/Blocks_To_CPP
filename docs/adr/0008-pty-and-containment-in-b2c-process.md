# ADR-0008: Our own PTY and containment layer in `b2c-process`

* Status: Proposed (M2 is built on it; the owner confirms the point marked *owner to confirm*)
* Date: 2026-10-05

## Context

M2 runs programs in a pseudo-terminal so that interactive programs behave as
in a console ([07 §7.6.2](../spec/07-toolchain-build-run.md#762-spawning)).
The spec named the `portable-pty` crate for this
([02 §2.1](../spec/02-architecture.md#21-technology-stack)). Two requirements
decide how the child is created:

* **Containment must start before the child runs any code.** On Windows the
  child must be in our Job Object (`KILL_ON_JOB_CLOSE`) before its first
  instruction, or a fast program can start a grandchild outside the job that
  survives *Stop*. That needs `CREATE_SUSPENDED`, then
  `AssignProcessToJobObject`, then `ResumeThread`. On Linux the child must be
  the leader of a new session with the PTY as its controlling terminal
  (`setsid` and `TIOCSCTTY` between `fork` and `exec`), and, when available,
  inside a cgroup v2 scope that also catches double-forked and `setsid`
  children.
* **Children inherit only what they need.** On Windows, pipe-mode children
  get exactly their three pipe ends through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`
  ([08 §8.7](../spec/08-security.md#87-process-execution-safety)).

`b2c-process` is the only crate allowed to contain `unsafe`, and every unsafe
block carries a `// SAFETY:` comment and CODEOWNERS review
([02 §2.3](../spec/02-architecture.md#23-repository-layout)).

Linux cgroup v2 scopes for an unprivileged user are created by the user's
systemd manager, which delegates the cgroup subtree. A process cannot simply
create a cgroup of its own in the shared hierarchy.

## Options considered

For the PTY:

1. **`portable-pty`.** Ready-made for both platforms. But it creates the
   process itself, without a suspended start or a job assignment before the
   child runs, and without an explicit handle list, so containment would be
   applied after the child is already running. It also brings its own
   dependency tree next to `rustix` and `windows-sys`, which we already use.
2. **Our own layer in `b2c-process` (chosen).** Linux: `openpty`, termios and
   the window size through `rustix`, with `setsid` and `TIOCSCTTY` in the
   `pre_exec` hook (async-signal-safe calls only). Windows:
   `CreatePseudoConsole`, then `CreateProcessW` with `STARTUPINFOEXW`
   (`PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`, or `HANDLE_LIST` in pipe mode),
   `CREATE_SUSPENDED`, Job Object assignment, then resume. Pipe mode is a
   fallback with the same API.
3. **A helper process** that sets up containment and then executes the
   program. One more binary to ship, sign and keep in step, for no gain over
   option 2.

For Linux cgroups:

1. **No cgroups.** A process group only. A program that daemonises survives
   *Stop*.
2. **`systemd-run --user --scope` (chosen).** The user manager creates a
   transient scope and runs the program in it; with `--scope` the program
   is executed in place, so its pid and PTY are kept.
3. **The D-Bus API of the user manager** (`StartTransientUnit`). It needs a
   D-Bus client crate and its dependency tree in the backend.
4. **Writing to cgroupfs directly.** It works only inside an already delegated
   subtree, which an ordinary desktop session does not give the app.

## Decision

* **Owner to confirm:** no `portable-pty`. PTY sessions are implemented in
  `b2c-process` as described in option 2, with `IoMode::{Pty, Pipes}` and
  `ContainmentLevel::{JobObject, Cgroup, ProcessGroupOnly}` reported to the
  UI ([07 §7.6.2](../spec/07-toolchain-build-run.md#762-spawning)).
* **cgroup v2 through `systemd-run`**, found at `/usr/bin/systemd-run` or
  `/bin/systemd-run` (an absolute path, never `PATH`), run as
  `systemd-run --user --scope --quiet --collect --unit=b2c-<build|run>-<ownerPid>-<16 hex> [limits] -- <program> <args>`.
  Builds add `-p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=32`; runs add no
  limits. `systemd-run` receives only `XDG_RUNTIME_DIR` and
  `DBUS_SESSION_BUS_ADDRESS` in addition to the program's own environment.
  *Stop* writes `1` to the scope's `cgroup.kill`, falling back to `SIGKILL` on
  every pid in `cgroup.procs`; `memory.events` reports out-of-memory kills.
* **Detection runs once at startup:** cgroup v2 controllers present,
  `systemd-run` present, `XDG_RUNTIME_DIR` set, and a trial scope succeeding
  within 5 s. Otherwise the level is `ProcessGroupOnly`, and the fallback is a
  process group plus `RLIMIT_AS` for compilers plus an RSS watchdog
  ([07 §7.5.2](../spec/07-toolchain-build-run.md#752-invocation)).
  `RLIMIT_AS` is never applied to programs, because it breaks
  AddressSanitizer.
* Stale `b2c-*` scopes whose owner process has died are killed at the next
  startup ([08 §8.14](../spec/08-security.md#814-residual-risks-accepted-documented-to-users)).
* The other platform helpers the backend needs also live in `b2c-process`,
  as safe wrappers in `b2c_process::os`: `atomic_replace` (rename with
  `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` on
  Windows), `harden_dll_search` (`SetDefaultDllDirectories`) and
  `open_https_url` (`xdg-open` by absolute path, or `ShellExecuteW`).
  `b2c-build` re-exports them as `b2c_build::os`, so the desktop app and the
  CLI need no direct dependency on `b2c-process`.

## Consequences

* More `unsafe` in `b2c-process`, all of it in platform modules with
  `// SAFETY:` comments, checked by Clippy's `undocumented_unsafe_blocks` and
  reviewed through CODEOWNERS. `cargo-geiger` reports it weekly.
* The ConPTY path cannot run in a Linux container. It is compiled by Clippy
  for `x86_64-pc-windows-gnu` in every lint job and tested on `windows-2025`
  runners; ConPTY behaviour differs between Windows 10 1809 and Windows 11,
  so its tests cover output that arrives after exit and a blocking
  `ClosePseudoConsole`.
* The cgroup path needs a user systemd instance. CI tests it in a dedicated
  job (`test-cgroup`, with lingering enabled); every other job exercises the
  fallback, and the cgroup tests there skip with a logged reason.
* Without cgroups, a program that daemonises can outlive *Stop*. The run
  status says "process group only", and the risk is documented
  ([08 §8.14](../spec/08-security.md#814-residual-risks-accepted-documented-to-users)).
* 02 §2.1, 02 §2.8, 07 §7.5.2 and 07 §7.6.2 describe this layer instead of
  `portable-pty`.
