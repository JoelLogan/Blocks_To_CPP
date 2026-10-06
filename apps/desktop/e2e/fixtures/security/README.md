# Fixtures of the E2E security tests

Project files that the [nightly security tests](../../specs/security/README.md) open. The tests
copy each one into a temporary folder before the app opens it.

| File                                         | Used by                                  | What it is                                                                                                                                                                                                                                                                                                                 |
| -------------------------------------------- | ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`trust-target.b2c`](trust-target.b2c)       | `restricted-ipc`, `outside-trust-change` | A valid program that prints one line, with one define (`GAME_LEVEL = 1`) that the tests change from outside the app.                                                                                                                                                                                                       |
| [`markup.b2c`](markup.b2c)                   | `markup`                                 | A valid, runnable program with HTML and script in its name, description, two string literals, a string variable, two block comments (one pinned open) and a note. Every payload would set the canary global `__b2cXss` if it ran, and would make an element with the attribute `data-b2c-xss` if it were parsed as markup. |
| [`markup-rejected.b2c`](markup-rejected.b2c) | `markup`                                 | A project the loader rejects (`B2C-E0110`) because of an unknown top-level key that is itself markup; its name is markup too. The start page quotes the key in the problems it lists.                                                                                                                                      |

The attack files of the malicious-project suite stay in
[`tests/security/projects`](../../../../../tests/security/projects/README.md); the tests read
them from there.
