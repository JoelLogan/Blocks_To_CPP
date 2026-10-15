# Developer guide

How the Blocks2Cpp desktop app is built inside, for contributors. To install the tools and build
and start the app, follow [`BUILDING.md`](../../BUILDING.md), the only place with those steps.
The rules for a change and the checks CI runs are in [CONTRIBUTING.md](../../CONTRIBUTING.md).

| Page                                                | What it covers                                                                                                                                                  |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [Desktop app architecture](desktop-architecture.md) | The backend adapter, the app shell, the block editor and the features, how the desktop crate fits the Cargo workspace, and the list of Blockly seams (ADR-0014) |
| [Security settings](desktop-security.md)            | The Content Security Policy, the isolation hook, capabilities, navigation, the frontend rules, Trusted Types and the Windows hardening                          |
| [Dependencies](dependencies.md)                     | Every npm package and crate the app added, with its version, licence and why; the tools; why some versions are held back                                        |
| [Testing](testing.md)                               | Where the tests are, the desktop crate's tests, the `e2e-hooks` feature and the end-to-end tests                                                                |
| [Recipes](recipes.md)                               | Adding an IPC command or a dependency, subclassing a Blockly class, the frontend in a browser, the icons, moving documentation                                  |

Elsewhere:

- [`apps/desktop/README.md`](../../apps/desktop/README.md): what the app is and its code map.
- [`packages/README.md`](../../packages/README.md): the frontend packages and the template they
  follow.
- [The specification](../spec/README.md): the architecture (02), the user interface (04), security
  (08) and the quality process (09); and the [decision records](../adr/README.md).
- [`apps/desktop/e2e/README.md`](../../apps/desktop/e2e/README.md): the end-to-end tests, their
  settings and the stable test IDs.
