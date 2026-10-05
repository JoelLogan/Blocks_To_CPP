# Architecture Decision Records

An ADR records one significant decision: its context, the options considered,
what was chosen and why, and the consequences. ADRs are never deleted. When a
decision changes, a new ADR supersedes the old one, and the old one is marked
`Superseded by ADR-NNNN`.

| ADR | Title | Status |
| ----- | ------- | -------- |
| [0001](0001-desktop-shell-tauri.md) | Desktop shell: Tauri 2 | Accepted |
| [0002](0002-block-editor-blockly.md) | Block editor: Blockly with the Zelos renderer | Accepted |
| [0003](0003-rust-core-native-and-wasm.md) | One Rust compiler core, built natively and as WebAssembly | Accepted |
| [0004](0004-project-format.md) | Project format: editor-independent, versioned JSON with array statement lists | Accepted |
| [0005](0005-no-compiler-flags-in-projects.md) | Project files never contain compiler flags, paths or commands | Accepted |
| [0006](0006-using-namespace.md) | `using namespace`: a project setting and a block, with exact name rules | Accepted |
| [0007](0007-backend-crates-and-ipc-contract.md) | Backend crates and a generated IPC contract | Proposed |
| [0008](0008-pty-and-containment-in-b2c-process.md) | Our own PTY and containment layer in `b2c-process` | Proposed |
| [0009](0009-e2e-tooling-and-test-seams.md) | End-to-end tooling under the pnpm policy, and the test seams | Proposed |
| [0010](0010-wasm-delivery-under-the-csp.md) | Delivering the WebAssembly core under the unchanged CSP | Accepted |
| [0011](0011-loose-blocks-in-m2.md) | Loose blocks stay errors, and loose stacks are saved intact | Proposed |

A *Proposed* ADR records a decision that work already proceeds on, with
points the owner still confirms or reverses (each marked *owner to confirm*).
A reversed point is changed in the ADR before it is accepted; after that, a
change needs a new ADR that supersedes it.

## Template

```markdown
# ADR-NNNN: <Title>

* Status: Proposed | Accepted | Superseded by ADR-XXXX
* Date: YYYY-MM-DD

## Context
What problem are we solving? What constraints apply?

## Options considered
1. Option A: pros / cons
2. Option B: pros / cons

## Decision
What we chose.

## Consequences
What becomes easier, what becomes harder, and what we must now do.
```
