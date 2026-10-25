# Grade average and maximum

An M3 acceptance assignment ([index](../README.md)). Status: pending, until
the blocks it needs arrive in wave W2.

## The assignment

Read grades from 0 to 100, one at a time, until the user types -1. Refuse a
grade outside 0 to 100 with a message and keep asking. Then print the grades
in the order they were typed, how many there are, their average with two
decimal places, the highest grade, and how many grades are above the
average. Print "No grades." when there are none.

## Concepts

A list (`std::vector`) that grows, a loop with a sentinel value, two passes
over the list, average and maximum.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| Each grade | *ask* (whole number) | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |
| The sentinel | *forever*, *leave loop*, *skip to next round* | [03 §3.7.5](../../../docs/spec/03-block-language.md#375-control-and-loops) | M2 |
| The list | *create* a list of int, *add … to*, *item … of*, *length of*, *is empty*, *for each* | [03 §3.7.8](../../../docs/spec/03-block-language.md#378-collections) | W2 |
| The average | *as double* | [03 §3.7.3](../../../docs/spec/03-block-language.md#373-math-and-logic) | M2 |
| Two decimal places | *show numbers with (2) decimal places* | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | W2 |

## Test data

Seven grades, one of them (105) refused: six remain, average 78.17,
highest 100, three above the average.
