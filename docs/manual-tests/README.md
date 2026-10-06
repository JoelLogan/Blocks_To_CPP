# Manual tests

Most of Blocks2Cpp is tested automatically
([09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy)): unit,
golden, property, fuzz and mutation tests, component tests with automated
accessibility checks, and end-to-end tests that drive the real app on Windows
and Linux. Some things no test can judge: what a screen reader really says,
or whether a person who has never seen the app finds their way. Those are
checked by hand at the end of each milestone, following the protocols here
([10 §10.1](../spec/10-roadmap.md#101-milestones)).

## Protocols

| Protocol                                                 | Milestone | What it checks                                                                                                                                                      | Status                                                                 |
| -------------------------------------------------------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| [M2 accessibility](m2-accessibility.md)                  | M2        | Screen readers (NVDA on Windows, Orca on Linux), the Tab order, the block editor's keys, colour, contrast, zoom, reflow and reduced motion in the real webviews     | Not run yet                                                            |
| [M2 usability ("without reading docs")](m2-usability.md) | M2        | At least three first-time users build and play the guessing game without documentation; and the exclusion review: nothing of M3–M6 has an entry point in the M2 app | Exclusion review run on the code; sessions and demo review not run yet |

M2 is declared done only once both are recorded in their sign-off tables.

## Running a protocol

- Test a build of a specific commit, and write that commit into the sign-off
  table. Start each run with a fresh profile, so no settings, trust records
  or recovery snapshots of earlier runs are left
  ([02 §2.7](../spec/02-architecture.md#27-persistence-locations) lists the
  folders).
- Several steps use the guessing game. The start page's templates are _Empty
  project_ and _Hello World_ only, so open `examples/guessing_game.b2c` from
  the repository with _Open…_, choose _Trust…_ in the Restricted Mode banner
  and then _Trust this project_, and save it anywhere (_Save as…_).
- Record the result in the protocol's own tables and file an issue for each
  failure, quoting the step. Participants and testers are named by role or
  ID, never by name, in the repository.
- When a protocol changes because the app changed, update it in the same
  pull request as the app.

## Not written yet

[09 §9.2](../spec/09-quality-and-delivery.md#92-testing-strategy) also asks
for manual checks of the High Contrast themes (they come in M5) and of a
real-world toolchain matrix (WinLibs, TDM-GCC, Scoop, Strawberry Perl's
GCC and distribution GCCs). CI covers GCC 11, 13 and 15 on Linux and MSYS2
UCRT64 on Windows; the toolchain matrix protocol is still to be written.
