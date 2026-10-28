# Temperature table

An M3 acceptance assignment ([index](../README.md)). Status: green since W2.

## The assignment

Ask for a start, an end and a step, in whole degrees Celsius. Print a table
that converts every temperature from the start to the end (both included)
to degrees Fahrenheit, F = C × 1.8 + 32, with one decimal place. Then print
how many rows the table has. Refuse a step below 1 with a message, and end
the program with exit code 1.

## Concepts

A counting loop with a step, decimal arithmetic, number formatting.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| The three numbers | *ask* (whole number) | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |
| The rows | *for … through … step* | [03 §3.7.5](../../../docs/spec/03-block-language.md#375-control-and-loops) | M2 |
| One decimal place | *show numbers with (1) decimal places* | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | W2 |
| A bad step | *stop program with exit code* | [03 §3.7.1](../../../docs/spec/03-block-language.md#371-program) | M2 |

## Test data

`stdin.txt` gives -10, 40 and 7: eight rows, from -10 °C to 39 °C, with
results such as 26.6 and 102.2 that need the decimal place.
