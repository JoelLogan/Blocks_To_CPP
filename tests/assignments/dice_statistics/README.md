# Dice statistics

An M3 acceptance assignment ([index](../README.md)). Status: pending, until
the blocks it needs arrive in wave W2.

## The assignment

Ask how many times to roll a die. Roll it that many times and count how
often each face comes up, in a map from the face to its count. Print each
face with its count and its share in percent (one decimal place), the
number of faces seen, the total of the counts, and the most common face
(the lowest one, if several are equally common).

## Concepts

Random numbers, a map, counting, percentages.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| The rolls | *random integer from (1) to (6)*, *repeat* | [03 §3.7.3](../../../docs/spec/03-block-language.md#373-math-and-logic) | M2 |
| The counts | a map from int to int, *set item … of*, *item … of*, *length of*, *for each key, value* | [03 §3.7.8](../../../docs/spec/03-block-language.md#378-collections) | W2 |
| The percentages | *show numbers with (1) decimal places* | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | W2 |

## Test data

600 rolls. The random blocks have no seed (spec 03 offers none, and the
helper seeds itself from `std::random_device`), so the counts differ on
every run: `stdout.regex` checks the shape of each line and the totals
that do not change (6 faces, 600 rolls), as the guessing game's golden
test does.
