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
| [`temperature_table`](temperature_table/README.md) | loops, formatting | W2 | pending |
| [`grade_average`](grade_average/README.md) | list (vector), average and maximum | W2 | pending |
| [`word_count`](word_count/README.md) | files, strings | W2 | pending |
| [`student_records`](student_records/README.md) | struct, list of structs, sorting | W2 | pending |
| [`bank_menu`](bank_menu/README.md) | `switch`, enum, `do … while` | W2 | pending |
| [`palindrome`](palindrome/README.md) | strings, reversal | W2 | pending |
| [`dice_statistics`](dice_statistics/README.md) | random numbers, map | W2 | pending |
| [`guessing_game`](guessing_game/README.md) | random numbers, input, loops | M2 | green |
| [`matrix_sum`](matrix_sum/README.md) | list of lists (vector of vectors) | W2 | pending |
| [`input_validation`](input_validation/README.md) | exceptions, text to number | W2 | pending |

Every run of the test prints the count, for example
`M3 assignments: 1 of 10 green`.

## The files

[`status.toml`](status.toml) lists every assignment as `pending` or
`green`, in the order of the index. Each assignment has a folder,
`tests/assignments/<name>/`:

- `README.md`: the assignment as a student would get it, its concepts, the
  blocks it needs (with the spec section and when each arrives) and its
  test data.
- `reference.cpp`: a reference solution. It uses only the standard library
  and only what M3's blocks can express (no lambdas or classes, which come
  in M4), written the way generated code looks. Each `std::cin >>` stands
  for an *ask* block: the input has one valid answer per line, never an
  empty line, and never runs out, so both print the same.
- The run data, as a golden test has it
  ([09 §9.2](../../docs/spec/09-quality-and-delivery.md#92-testing-strategy)):
  `stdin.txt`, `stdout.txt` (or `stdout.regex` when the output is random),
  `exit_code.txt`, and any input file the program reads. The program runs
  with this folder as its working directory, which stands in for the
  project folder a program runs in by default, so it names its files with
  relative paths (decision M3-D3). In a `stdout.regex` the file's own last
  line break is ignored, so the pattern ends with `\n`.

## What the test checks

[`crates/b2c-build/tests/assignments.rs`](../../crates/b2c-build/tests/assignments.rs)
checks every assignment in `status.toml`:

- **pending**: the folder holds all of the above, and `tests/golden/<name>/`
  does not exist. The reference solution is built with g++ the way the
  debug configuration builds generated code (C++20, `-Wall -Wextra
  -Wpedantic`, the sanitizers where the compiler has them, hardening),
  plus `-Werror`, and run in the folder: its output and exit code must be
  the expected ones. This checks the run data until the blocks exist. Once
  `examples/<name>.b2c` exists, it is built and run the same way as a
  golden example; when it passes, the test fails with "now passes: mark it
  green", so progress is never silent. Until then the test lists it as
  "does not pass yet" and carries on.
- **green**: `examples/<name>.b2c` exists and its run data is in
  `tests/golden/<name>/`, where the golden test also compares the
  generated C++; the folder here keeps only `README.md` and, if there is
  one, `reference.cpp`, so no run data exists twice. The project must pass
  the golden run check: no error or warning from the front end or g++, then
  the expected output and exit code. The reference solution, if any, must
  pass too.
- The index above has a row for every assignment, with its status in the
  last column, and every folder here is listed in `status.toml`.

Without g++ on `PATH` the build-and-run part is skipped, as in the golden
test, unless `B2C_REQUIRE_GXX` is set. CI sets it and runs the test on Linux
and Windows (MSYS2 UCRT64), and in the GCC 11 and GCC 15 containers
([`ci.yml`](../../.github/workflows/ci.yml)).

```sh
B2C_REQUIRE_GXX=1 cargo test -p b2c-build --test assignments
```

## Turning an assignment green

1. Build `examples/<name>.b2c` from blocks until the assignment test fails
   with "now passes: mark it green".
2. Move the run data to the golden test, every file but `README.md` and
   `reference.cpp`:

   ```sh
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
   `B2C_REQUIRE_GXX=1 cargo test -p b2c-build --test assignments --test golden`.

The golden test then covers it like every other example: the generated C++,
generating twice, and the run, on every compiler in CI.
