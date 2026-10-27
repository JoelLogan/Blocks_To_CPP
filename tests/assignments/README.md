# M3 acceptance assignments

M3 is done when a typical intro-CS course's assignments can be completed
with blocks, each with a golden test
([10 §10.1](../../docs/spec/10-roadmap.md#m3-language-breadth-i)). The owner
chose these ten (decision M3-D1,
[next steps §3.3](../../docs/plans/next-steps.md#33-decisions-before-m3-starts)).
Most need blocks that arrive in wave W2, so each one is written first as an
acceptance test: the assignment, a reference solution in plain C++, and the
run data that its block project must reproduce later. The test turns them
green one by one as the block projects arrive.

## Index

| Assignment | Concepts | Wave | Status |
| --- | --- | --- | --- |
| [`temperature_table`](temperature_table/README.md) | loops, formatting | W2 | green |
| [`grade_average`](grade_average/README.md) | list (vector), average and maximum | W2 | green |
| [`word_count`](word_count/README.md) | files, strings | W2 | green |
| [`student_records`](student_records/README.md) | struct, list of structs, sorting | W2 | green |
| [`bank_menu`](bank_menu/README.md) | `switch`, enum, `do … while` | W2 | pending |
| [`palindrome`](palindrome/README.md) | strings, reversal | W2 | green |
| [`dice_statistics`](dice_statistics/README.md) | random numbers, map | W2 | green |
| [`guessing_game`](guessing_game/README.md) | random numbers, input, loops | M2 | green |
| [`matrix_sum`](matrix_sum/README.md) | list of lists (vector of vectors) | W2 | green |
| [`input_validation`](input_validation/README.md) | exceptions, text to number | W2 | green |

When g++ is on `PATH`, every run of the test prints the count, for example
`M3 assignments: 1 of 10 green`.

## The files

[`status.toml`](status.toml) lists every assignment as `pending` or
`green`. The index above has one row for each, in the same order. Each
assignment has a folder, `tests/assignments/<name>/`:

- `README.md`: the assignment as a student would get it, its concepts, the
  blocks it needs (with the spec section and when each arrives) and its
  test data.
- `reference.cpp`: a reference solution. It uses only the standard library
  and only what M3's blocks can express (no lambdas or classes, which come
  in M4), close to the code the blocks generate. The test compares what it
  prints, not its text, so a block project may take another shape where the
  blocks need one; the assignment's README says where. Each `std::cin >>`
  stands for an *ask* block: the input has one valid answer per line, never
  an empty line, and never runs out, so both print the same.
- The run data, as a golden test has it
  ([09 §9.2](../../docs/spec/09-quality-and-delivery.md#92-testing-strategy)):
  `stdin.txt`, `stdout.txt` (or `stdout.regex` when the output is random),
  `exit_code.txt`, and any input file the program reads. The program runs
  in a copy of this folder, which stands in for the project folder a
  program runs in by default, so it names its files with relative paths
  (decision M3-D3). A program that adds, removes or changes a file in the
  copy fails, so no run leaves anything in the source tree for a later run
  to find. In a `stdout.regex` the file's own last line break is ignored,
  so the pattern ends with `\n`.
- `<name>.b2c`, the block project, while it is being built. It stays here
  until it passes, because every file in
  [`examples/`](../../examples/README.md) must already pass the tests that
  read all examples: no diagnostic at all (notes included), C++ that
  compiles with `-Werror`, the canonical form, `b2c check`, `b2c migrate`
  and the editor's round trip.

## What the test checks

[`crates/b2c-build/tests/assignments.rs`](../../crates/b2c-build/tests/assignments.rs)
checks every assignment in `status.toml`:

- **pending**: the folder holds the README, the reference solution and the
  run data, and neither `tests/golden/<name>/` nor `examples/<name>.b2c`
  exists. The reference solution is built with g++ the way the debug
  configuration builds generated code (C++20, `-Wall -Wextra -Wpedantic`,
  the sanitizers where the compiler has them, hardening), plus `-Werror`,
  and run on the run data: its output and exit code must be the expected
  ones. This checks the run data until the blocks exist. If `<name>.b2c`
  is in the folder, it is built and run the same way as a golden example.
  When it passes and is ready for `examples/` (no diagnostic at all, notes
  included, and the canonical form that `b2c fmt` writes), the test fails
  with "now passes: mark it green", so progress is never silent. Until
  then the test lists it as "does not pass yet" or "not ready for
  examples/ yet", with the first reason, and carries on.
- **green**: `examples/<name>.b2c` exists and its run data is in
  `tests/golden/<name>/`, where the golden test builds and runs it and
  compares the generated C++. The folder here keeps only `README.md` and,
  if there is one, `reference.cpp`, so nothing exists twice; the reference
  solution, if any, must pass on the golden run data.
- The index above has exactly one row for each assignment in
  `status.toml`, in the same order, with its status in the last column,
  and every folder here is listed in `status.toml`.

Both tests build and run programs with the same helpers
([`common/run_data.rs`](../../crates/b2c-build/tests/common/run_data.rs)),
so a project that passes here passes the golden test's run check too.
Without g++ on `PATH` the build-and-run part is skipped, as in the golden
test, unless `B2C_REQUIRE_GXX` is set. CI sets it and runs the test on Linux
and Windows (MSYS2 UCRT64), and in the GCC 11 and GCC 15 containers
([`ci.yml`](../../.github/workflows/ci.yml)).

```sh
B2C_REQUIRE_GXX=1 cargo test -p b2c-build --test assignments
```

## Turning an assignment green

1. Build `tests/assignments/<name>/<name>.b2c` from blocks (it may be
   committed while it is unfinished) until the assignment test fails with
   "now passes: mark it green".
2. Move the project to the examples and the run data to the golden test,
   every file but `README.md` and `reference.cpp`:

   ```sh
   git mv tests/assignments/<name>/<name>.b2c examples/
   mkdir tests/golden/<name>
   git mv tests/assignments/<name>/{stdin.txt,stdout.txt,exit_code.txt} tests/golden/<name>/
   ```

   with `stdout.regex` instead of `stdout.txt` where there is one, and any
   input file (such as `poem.txt`).
3. Write the generated C++ with
   `B2C_UPDATE_GOLDEN=1 cargo test -p b2c-build --test golden`, and review
   `tests/golden/<name>/main.cpp`.
4. Change the assignment to `"green"` in `status.toml` and in the index's
   last column, and add the example to
   [`examples/README.md`](../../examples/README.md).
5. Check with
   `B2C_REQUIRE_GXX=1 cargo test -p b2c-build --test assignments --test golden`,
   then run the other tests that read every example:
   `cargo test -p b2c-lang -p b2c-cli -p b2c-app -p b2c-catalog` and
   `pnpm --filter @blocks2cpp/desktop exec vitest run src/editor/sync/roundTrip.test.ts`.

The golden test then covers it like every other example: the generated C++,
generating twice, and the run, on every compiler in CI.
