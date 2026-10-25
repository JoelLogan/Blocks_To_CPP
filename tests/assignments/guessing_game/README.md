# Number guessing with retries

An M3 acceptance assignment ([index](../README.md)). Status: green since M2.

## The assignment

Pick a random whole number from 1 to 100. Ask the user to guess it until
they get it right; after each guess, say whether it was too low, too high
or correct.

## Concepts

Random numbers, input, a loop that repeats until a condition holds, an
if/else-if/else chain.

## Blocks

The worked example of [03 §3.13.1](../../../docs/spec/03-block-language.md#3131-guessing-game-input-random-loops-branching):
*random integer*, *ask* (whole number), *repeat until* and *if / else if /
else*, all in the catalog since M2.

## Files

The block project is [`examples/guessing_game.b2c`](../../../examples/guessing_game.b2c),
and its run data and generated C++ are the golden test's, in
[`tests/golden/guessing_game/`](../../golden/guessing_game/main.cpp). It has
no hand-written reference solution: the generated `main.cpp` is the
solution. The input guesses 1, 2, 3 and so on, so the output is matched by
a regular expression.
