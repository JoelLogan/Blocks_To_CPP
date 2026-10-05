# The `b2c` command-line tool

`b2c` runs the same pipeline as the app (load → check → generate C++ →
compile with g++ → run) from a terminal, a script or CI. It uses the same
limits, compiler arguments and safety rules as the app
([spec §7.9](../spec/07-toolchain-build-run.md#79-command-line-interface)).

```text
b2c check      <project.b2c> [--format text|json]
b2c generate   <project.b2c> --out <dir> [--export]
b2c build      <project.b2c> [--config debug|release] [--toolchain <g++>] [--out <file>] [--cache-dir <dir>]
b2c run        <project.b2c> [--config debug|release] [--toolchain <g++>] [--stdin <file>]
                             [--timeout <duration>] [--cache-dir <dir>] [-- <program args>…]
b2c toolchains [--format text|json]
b2c migrate    <project.b2c> [--in-place]
b2c fmt        <project.b2c> [--check]
```

> **Trust:** the command line does not use the app's Restricted Mode.
> Running `b2c build` or `b2c run` on a file is your explicit decision to
> compile and run it, like running `make`. Only build projects you trust.

## Commands

### `b2c check`

Loads the project, checks every block against the block catalog and runs the
analyser. It prints every problem and exits with `0` when there are no
errors (warnings are allowed) or `1` when there are.

### `b2c generate`

Writes the generated C++ files into the `--out` folder, creating it if
needed. Each file is rewritten only when its content changed. `--export`
leaves out the "edit the blocks, not this file" banner. The project must have
no errors. The C++ is always indented with 4 spaces: `b2c` does not read the
app's code style setting.

### `b2c build`

Generates C++ and compiles it with g++ into the build cache, then prints the
path of the executable. Nothing is recompiled when neither the generated C++
nor the compiler command changed. Errors and warnings are printed; notes
(such as a sanitizer the compiler does not offer) are left out. `--out` also copies the executable to a path of your
choice (atomically, never through a symbolic link, and executable). `--toolchain` picks a g++ by absolute path; otherwise `b2c` uses the
first suitable g++ it finds (see `b2c toolchains`). `--cache-dir` keeps the
build cache in another folder instead of the per-user one. By default builds
go to `%LOCALAPPDATA%\Blocks2Cpp\builds\` on Windows and to
`$XDG_CACHE_HOME/blocks2cpp/builds/` (normally `~/.cache/blocks2cpp/builds/`)
on Linux, the same cache the app uses
([spec §2.7](../spec/02-architecture.md#27-persistence-locations)). Earlier
versions used `%LOCALAPPDATA%\Blocks2Cpp\cache\` on Windows; that folder is
no longer used and can be deleted. `b2c` never deletes entries from the cache;
the app removes old ones. Builds made by `b2c` and by the app are kept apart,
because only the app's builds include its console helpers.
Compiler errors are mapped back to the blocks that produced the code.

### `b2c run`

Builds the project (reusing the cache when nothing changed) and runs it. The
program's output goes straight to your terminal. Its input comes from your
terminal, or from a file with `--stdin`. `--timeout` stops the program after
a while, for example `500ms`, `10s` or `2m` (from 1ms up to 24h). Everything after `--` is passed
to the program as its arguments.

### `b2c toolchains`

Lists the g++ compilers found on this computer, with their version, target,
supported C++ standards and any problems. Checking a compiler builds a few
tiny test programs, so the results are kept in `toolchains.json` and checked
again only when the compiler file changes. The file is shared with the app:
it is in `%LOCALAPPDATA%\Blocks2Cpp\` on Windows and in
`$XDG_CONFIG_HOME/blocks2cpp/` (normally `~/.config/blocks2cpp/`) on Linux.

### `b2c migrate`

Upgrades a project file written by an older version of Blocks2Cpp to the
current format. It prints the result, or rewrites the file with `--in-place`.
Printed to a terminal, control and invisible characters in project text are
shown as JSON `\uXXXX` escapes (the same JSON value); piped output is the
file exactly.

### `b2c fmt`

Rewrites the project file in its canonical form: the same layout the app
saves, so diffs stay small. `--check` only reports whether the file is
already formatted (exit code `1` if not), which is useful in CI.

## Exit codes

| Code | Meaning |
| ------ | --------- |
| `0` | Success |
| `1` | The project has errors (or, for `fmt --check`, is not formatted) |
| `2` | Usage problem: bad arguments, or a file that cannot be read or written |
| `3` | Toolchain problem: no suitable g++ found, it is broken or cannot be started, or it crashed |

`b2c run` returns the **program's own exit code**, except:

| Code | Meaning |
| ------ | --------- |
| `124` | The program was stopped by `--timeout` |
| `125` | The program could not be built or started (the reason is printed) |
| `128 + N` | Linux: the program was killed by signal `N` (for example `139` for a segmentation fault) |

## JSON output

`b2c check --format json` prints one JSON object on one line:

```json
{
  "version": 1,
  "file": "examples/hello_world.b2c",
  "ok": true,
  "diagnostics": []
}
```

* `version` is the format version. It changes only when the format changes
  in an incompatible way; new fields may be added at any time.
* `ok` is `false` when there is at least one error.
* Each diagnostic has `code` (for example `B2C-E0201`, documented in
  [diagnostics/](diagnostics/README.md)), `severity` (`error`, `warning` or
  `info`), `message`, `primary` (the location: optional `module` and `block`
  IDs and a `part`), optional `related` locations, `source` (the stage that
  found it) and, for compiler messages, the original text in `raw`.

## Safety

* Project files are untrusted input. `b2c` reads at most the project size
  limit plus one byte, and every text it prints from a project has control
  and invisible formatting characters escaped, so a project cannot send
  commands to your terminal.
* Files are written atomically (a temporary file renamed into place) and
  `b2c` refuses to write through symbolic links.
* The compiler runs with a minimal, fixed environment and a time limit; see
  [spec §7.5.2](../spec/07-toolchain-build-run.md#752-invocation).
