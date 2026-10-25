# Palindrome and string reversal

An M3 acceptance assignment ([index](../README.md)). Status: pending, until
the blocks it needs arrive in wave W2.

## The assignment

Ask for words or sentences until the user types `quit`. For each one, print
it reversed and say whether it is a palindrome: whether it reads the same
backwards when spaces and letter case are ignored. At the end, print how
many of the texts were palindromes. Write the reversal and the check as
functions.

## Concepts

Text as a sequence of characters, indexing, building text, functions that
take and return text.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| The texts | *ask* (text), *forever*, *leave loop* | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |
| Reversing | *letter … of*, *length of*, *for … down to*, *join* | [03 §3.7.4](../../../docs/spec/03-block-language.md#374-text) | W2 (*join*: M2) |
| Ignoring case | *lowercase* | [03 §3.7.4](../../../docs/spec/03-block-language.md#374-text) | W2 |
| The functions | *define* with text parameters and results | [03 §3.7.7](../../../docs/spec/03-block-language.md#377-functions) | M2 |

## Test data

Six texts: "racecar", "Never odd or even" and "Step on no pets" (spaces and
capitals ignored) and "A" are palindromes; "Blocks to C++" and "hello" are
not.
