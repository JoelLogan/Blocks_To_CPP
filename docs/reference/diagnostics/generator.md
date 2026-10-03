# Generating C++ (`B2C-E07xx`)

These problems come from turning the analysed program into C++
([spec §6.8](../../spec/06-compiler-pipeline.md#68-stage--desugaring-and-emission)).
Blocks the analyser accepts should always generate complete C++, so most of
these point at a bug in Blocks2Cpp rather than a mistake in your blocks.

## B2C-E0701: part of the program could not be turned into C++

**Severity:** error · **Source:** generator

**What it means:** the code generator needed a placeholder (`0 /* error */`,
`/* error */;` or an `int /* error */` type) for part of a program that
passed every check. Code with a placeholder would still compile, but it would
not do what the blocks say, so Blocks2Cpp refuses to build it.

**Example message:** *Part of this program could not be turned into C++, so
it was not built. This looks like a bug in Blocks2Cpp; please report it with
the project file.*

**How to fix:** this is almost always a bug in Blocks2Cpp. Please report it
(see [SECURITY.md](../../../SECURITY.md) if the project could be used to
attack someone, otherwise open an issue) and attach the project file. As a
workaround, look for very deeply nested blocks or expressions and split them
into smaller pieces, for example by storing a part in a variable first.
