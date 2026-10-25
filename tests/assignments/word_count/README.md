# Word and line count of a file

An M3 acceptance assignment ([index](../README.md)). Status: pending, until
the blocks it needs arrive in wave W2.

## The assignment

Ask which word to count. Then read the file `poem.txt` line by line and
print how many lines, words and characters (without the line breaks) it
has, which line is the longest (the first one, if several are), and how
often the chosen word appears. Words are separated by spaces; two spaces in
a row do not make an empty word. A word counts only when it matches exactly,
letter case included. If the file cannot be opened, say so on the error
stream and end with exit code 1.

## Concepts

Reading a text file line by line, splitting text, string comparison,
counting.

## Blocks

| For | Block | Spec | In the catalog |
| --- | --- | --- | --- |
| The word | *ask* (text) | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |
| The file | *for each line … in file ("poem.txt") … if it can't be opened* | [03 §3.7.10](../../../docs/spec/03-block-language.md#3710-files) | W2 |
| The words | *split … by (" ")*, *length of*, text comparison | [03 §3.7.4](../../../docs/spec/03-block-language.md#374-text) | W2 |
| Each word | *for each* over a list | [03 §3.7.8](../../../docs/spec/03-block-language.md#378-collections) | W2 |
| The error | *print … to the error stream*, *stop program with exit code* | [03 §3.7.6](../../../docs/spec/03-block-language.md#376-input--output) | M2 |

## Files

The program reads one file, `poem.txt`, which it names with a path relative
to the folder it runs in, and writes none (decision M3-D3: a program
touches only the files its blocks name). The test runs it in the folder
that holds its run data, as a project runs in its project folder
by default ([07 §7.6.2](../../../docs/spec/07-toolchain-build-run.md#762-spawning)). To run
`examples/word_count.b2c` in the app, save the project in a folder that
holds a `poem.txt`.

## Test data

`poem.txt` has 5 lines (one empty, one with two spaces between words),
22 words and 98 characters; `stdin.txt` asks for "the", which appears
4 times ("The" does not count).
