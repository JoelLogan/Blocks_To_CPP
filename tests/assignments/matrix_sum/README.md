# Matrix sum

An M3 acceptance assignment ([index](../README.md)). Status: pending, until
the blocks it needs arrive in wave W2.

## The assignment

Ask for the number of rows and columns of a matrix of whole numbers, then
for each number in turn, row by row. Store the matrix as a list of rows.
Print it one row per line followed by the row's sum, then the sum of each
column and the total. Refuse a matrix without rows or columns, with exit
code 1.

## Concepts

A list of lists (`std::vector<std::vector<int>>`), nested loops, row and
column sums.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| The matrix | a list of lists of int, *add … to*, *item … of*, *set item … of*, *for each* | [03 §3.7.8](../../../docs/spec/03-block-language.md#378-collections) | W2 |
| The prompts | *ask* (whole number) with a *join*ed prompt | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |
| A row on one line | *print* without a new line | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |

## Test data

A matrix of 3 rows and 4 columns, with negative numbers, that is not
square, so mixing up rows and columns shows: row sums 9, 17 and 6, column
sums -1, 5, 19 and 9, total 32.
