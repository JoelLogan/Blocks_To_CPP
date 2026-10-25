# Menu with switch and an enum

An M3 acceptance assignment ([index](../README.md)). Status: pending, until
the blocks it needs arrive in wave W2.

## The assignment

Write a small bank account: show a menu (deposit, withdraw, show balance,
quit) and ask for a choice until the user quits. Turn the typed number into
a value of an enum of actions in a function that uses `switch`, then act on
the action with a second `switch`. Refuse amounts of 0 or less, refuse to
withdraw more than the balance, and answer an unknown choice with a hint.

## Concepts

An enum, `switch` on a number and on an enum, a `do … while` loop, a
function that returns an enum value.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| The actions | *enum* with values, *Action :: Deposit* | [03 §3.7.9](../../../docs/spec/03-block-language.md#379-types-structs-classes-enums) | W2 |
| The choices | *switch on … case … default* | [03 §3.7.5](../../../docs/spec/03-block-language.md#375-control-and-loops) | W2 |
| The menu loop | *do … while* | [03 §3.7.5](../../../docs/spec/03-block-language.md#375-control-and-loops) | W2 |
| The conversion | *define … returns* the enum type | [03 §3.7.7](../../../docs/spec/03-block-language.md#377-functions) | M2 (the enum type: W2) |
| Choices and amounts | *ask* (whole number) | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |

## Test data

Eight choices: show the balance, deposit 50, withdraw 80 (refused), withdraw
20, choice 7 (unknown), deposit -5 (refused), show the balance (30), quit.
