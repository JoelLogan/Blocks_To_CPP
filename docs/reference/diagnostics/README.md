# Diagnostics reference

Every problem Blocks2Cpp reports has a stable code. This folder documents each
one: what it means, an example, and how to fix it. The app links each diagnostic
to its entry here, and CI (`tools/check-diagnostics-docs.py`) fails if a code
used in the sources has no entry.

| Codes | Area | Reference |
|-------|------|-----------|
| `B2C-E01xx` | Loading the project file | [loader-and-catalog.md](loader-and-catalog.md) |
| `B2C-E06xx` | Checking blocks against the catalog | [loader-and-catalog.md](loader-and-catalog.md) |
| `B2C-E02xx`, `B2C-E03xx`, `B2C-E04xx`, `B2C-W05xx`, `B2C-I05xx` | Names, types, structure and lints | [analyser.md](analyser.md) |
| `B2C-E07xx` | Generating C++ | [generator.md](generator.md) |
| `B2C-T1xxx` | Finding and using the compiler | [toolchain.md](toolchain.md) |

Severity: **E** errors block building, **W** warnings point at likely mistakes,
**I** notes are informational. The code ranges follow
[spec §6.12](../../spec/06-compiler-pipeline.md#612-diagnostics-model).

## Messages from g++ and the linker (`C:*`)

When g++ or the linker reports something, Blocks2Cpp shows it on the block
that produced the code, using the source map
([spec §7.5.3](../../spec/07-toolchain-build-run.md#753-diagnostics-capture-and-mapping)).
These messages have codes starting with `C:` instead of `B2C-`:

| Code | Meaning |
|------|---------|
| `C:-W<name>` | A g++ warning or error controlled by that option, for example `C:-Wunused-variable` |
| `C:error` | A g++ error with no option |
| `C:link` | A linker error, for example an undefined reference |
| `C:limit` | The compiler ran out of time or memory |
| `C:failed` | The compiler failed without saying why (its output is attached) |
| `C:crashed` | g++ itself crashed (an internal compiler error). This is a bug in that g++ release, not in the project; `b2c` exits with `3`. Builds first retry with plain-text messages, which avoids known crashes in GCC 13's SARIF output |
| `C:truncated` | There were more messages than can be shown |

The original compiler text is always attached. Blocks the analyser accepts
should never produce C++ that g++ rejects, so a `C:` error is labelled as a
probable bug in Blocks2Cpp; please report it with the project file.

