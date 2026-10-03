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
