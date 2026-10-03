# Example projects

Each example is a complete Blocks2Cpp project (`.b2c`). They double as the
golden end-to-end tests (spec §9.2): CI generates C++ from each one, compiles
it with g++, runs it with the input in `tests/golden/<name>/stdin.txt` and
compares the output and exit code with the expected files in that folder.

| File | Name | What it shows |
|------|------|---------------|
| [`hello_world.b2c`](hello_world.b2c) | Hello World | The smallest program: one print block. |
| [`sum_to_ten.b2c`](sum_to_ten.b2c) | Sum to Ten | A variable, a counting loop and change-by. |
| [`guessing_game.b2c`](guessing_game.b2c) | Guessing Game | Random numbers, asking for input, repeat-until and if/else-if/else (spec §3.13.1). The input guesses 1, 2, 3, … so every answer is 'Too low!' until it is correct. |
| [`fizzbuzz.b2c`](fizzbuzz.b2c) | FizzBuzz | The remainder operator and an else-if chain built from nested comparison blocks. |
| [`factorial.b2c`](factorial.b2c) | Factorial | A recursive function with a parameter and a return value. |
| [`temperature.b2c`](temperature.b2c) | Temperature Converter | Decimal numbers, asking for a number and arithmetic in an expression slot. |
| [`greeting.b2c`](greeting.b2c) | Greeting | Asking for a whole line of text and joining text. |
| [`countdown.b2c`](countdown.b2c) | Countdown | Counting down with a for loop. |
| [`times_table.b2c`](times_table.b2c) | Times Table | Nested loops; print without a new line, with separators. |
| [`primes.b2c`](primes.b2c) | Prime Numbers | A function returning true/false with an early return inside a while loop. |
| [`max_of_three.b2c`](max_of_three.b2c) | Largest of Three | A function with three parameters, and/or conditions and an else-if chain. |
| [`weather.b2c`](weather.b2c) | Weather Advice | True/false values, and/or/not blocks and the conditional value block. |
| [`average.b2c`](average.b2c) | Average | Repeat a number of times, decimal arithmetic and an explicit conversion. |
| [`exit_code.b2c`](exit_code.b2c) | Exit Code | Stopping the program with an exit code from inside a function. |
| [`tricky_text.b2c`](tricky_text.b2c) | Tricky Text | Text that must be escaped correctly in C++ (spec §8.4.2), and a comment ending in a backslash that must not hide the next line (§8.4.3). |
