# @blocks2cpp/ipc-types

The IPC contract between the Blocks2Cpp editor and its backend, for TypeScript
([docs/spec/02-architecture.md §2.5](../../docs/spec/02-architecture.md#25-ipc-surface)): every
request, response, channel message and error type, the command names, `IPC_VERSION`, and a typed
client with one method per command.

The types also include the pipeline's shared shapes (`Diagnostic`, `GeneratedFile`, `SourceMap`,
`StaticType`, `SymbolInfo` and their parts), which the WebAssembly core returns too: the Rust copies
in `b2c_ipc::diag` and `b2c_ipc::pipeline` serialise byte for byte like the `b2c-ir` types, so other
packages import these declarations instead of keeping their own.

Everything in `src/generated/` is generated from the Rust crate
[`crates/b2c-ipc`](../../crates/b2c-ipc/src/lib.rs). Never edit it by hand; change the Rust types
and regenerate:

```sh
B2C_UPDATE_IPC=1 cargo test -p b2c-ipc --features ts --test generate
```

Without `B2C_UPDATE_IPC=1` the same test fails when a committed file is stale. The generator also
writes the isolation hook's allowlist (`apps/desktop/src-tauri/isolation/allowlist.generated.js`)
and its test samples, so the client, the allowlist and the backend cannot drift apart.

## Use

```ts
import { createIpcClient, IpcCallError } from '@blocks2cpp/ipc-types';

const ipc = createIpcClient(transport); // Tauri's invoke and Channel, or a fake in tests
const { buildId } = await ipc.buildStart({ handle, document, config: 'debug' }, (event) => {
  // progress, diagnostics, then exactly one finished event
});
```

`IpcTransport` has two functions: `invoke(cmd, args)` and `channel(onMessage)`. The desktop app
implements it with `invoke` and `Channel` from `@tauri-apps/api/core`. Every client method rejects
with an `IpcCallError` whose `error` is either the command's typed `IpcError` (`{ code, … }`) or
`{ code: 'transport', message }`. User-facing text comes from the frontend's messages, keyed by the
error code.

The package has no runtime dependencies. It is consumed as TypeScript source (`exports` points at
`src/index.ts`), so the app's bundler compiles it.

## Checks

```sh
pnpm --filter @blocks2cpp/ipc-types run typecheck
pnpm --filter @blocks2cpp/ipc-types run lint
pnpm --filter @blocks2cpp/ipc-types run format:check
pnpm --filter @blocks2cpp/ipc-types run test
```
