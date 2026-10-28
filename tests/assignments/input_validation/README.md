# Input validation with exceptions

An M3 acceptance assignment ([index](../README.md)). Status: green since W2.

## The assignment

Ask for the user's age as text until it is valid. Write a function that
turns the text into an age: the conversion to a number throws when the text
is not a number or the number is too big, and the function itself throws a
runtime error, with a message, for a negative age or one over 150. Catch
each kind of error and print a message: your own for the conversion's
errors, the error's message for the runtime error. Then print the age in
ten years and how many tries it took.

## Concepts

Exceptions: throwing, catching by type, the error's message; converting
text to a number.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| The answer | *ask* (text), *repeat until* | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |
| The conversion | *text … as number (int)* (in the Text category) | [03 §3.7.3](../../../docs/spec/03-block-language.md#373-math-and-logic) | W2 |
| The errors | *try … catch* (invalid argument, out of range, runtime error), *throw (runtime error) with message*, *message of* | [03 §3.7.11](../../../docs/spec/03-block-language.md#3711-errors) | W2 |
| The message | *join* text and a number | [03 §3.7.4](../../../docs/spec/03-block-language.md#374-text) | M2 |

The conversion is `std::stoi`, which reads a number at the start of the
text and ignores the rest, so `12abc` gives 12. The test data avoids such
input; the assignment is about the exceptions, not about a strict parser.

## Test data

Five answers: "twelve" (not a number), 99999999999 (too big), -5 and 200
(the function's own errors), then 42.
