# Student records sorted by mark

An M3 acceptance assignment ([index](../README.md)). Status: green since W2.

## The assignment

Define a record for a student with a name and a mark. Ask how many
students there are, then each one's full name and mark. Sort the records
from the highest mark to the lowest, print them as a numbered ranking, and
print how many students passed (a mark of 50 or more).

## Concepts

A struct, a list of structs, fields, a function that changes a list it is
given, sorting.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| The record | *struct* with fields, *new … with*, *… of* (a field of a struct value) | [03 §3.7.9](../../../docs/spec/03-block-language.md#379-types-structs-classes-enums) | W2 |
| The list | a list of the struct, *add … to*, *item … of*, *set item … of*, *length of*, *for each* | [03 §3.7.8](../../../docs/spec/03-block-language.md#378-collections) | W2 |
| The sort | *define* with an editable parameter, nested *for* loops | [03 §3.7.7](../../../docs/spec/03-block-language.md#377-functions) | M2 (the struct type: W2) |
| Names and marks | *ask* (text, whole number) | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |

The reference sorts by hand (a selection sort). The *sort … by* block needs
a comparison function, a lambda, and lambdas come in M4
([10 §10.1](../../../docs/spec/10-roadmap.md#m4-language-breadth-ii)). No two
marks in the test data are equal, so any correct sort gives the same
ranking.

The sort compares `students.at(j).mark`: in blocks, *mark of (item (j) of
[students])*. The field block takes its struct as a value input (`OBJ`, the
struct type bound from what is plugged in), so it reads a field of an *item*
block directly
([03 §3.7.9](../../../docs/spec/03-block-language.md#379-types-structs-classes-enums)).

## Test data

Five students with full names (each read as a whole line) and marks 74, 91,
58, 45 and 88: the ranking starts with 91, and 4 of 5 passed.
