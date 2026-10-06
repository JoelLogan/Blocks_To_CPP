# Nightly E2E security tests

The security tests of [08 §8.13](../../../../../docs/spec/08-security.md#813-continuous-security-process),
run in the real app on Linux and Windows every night (the `e2e-security` job of
[`nightly.yml`](../../../../../.github/workflows/nightly.yml)) and on demand. They drive the app
the way script injected into the editor would: from the webview, through Tauri's internal
`window.__TAURI_INTERNALS__` (present in every build, whatever `withGlobalTauri` says), never
through the app's own IPC client or the test hook. Only the native dialogs are scripted, through
the app's `e2e-hooks` seam ([ADR-0009](../../../../../docs/adr/0009-e2e-tooling-and-test-seams.md)).
Any abuse the app accepts fails the run.

Each spec checks one or two items of 08 §8.13:

- **1**, [`malicious-projects.e2e.ts`](malicious-projects.e2e.ts): every file of
  [`tests/security/projects`](../../../../../tests/security/projects/README.md) is rejected with
  exactly the codes its table lists, or opens in Restricted Mode (`noRecord`); building each one
  (both configurations) is refused with `restricted`; no compiler process starts and the build
  cache stays empty. Through the start page, a rejected file shows its problems, an accepted one
  shows the banner, and F5 and Run start nothing.
- **2**, [`restricted-ipc.e2e.ts`](restricted-ipc.e2e.ts): `build_start` and `run_start` on a
  restricted project are refused with `restricted`. Trusted, the same calls build and run (the
  compiler watch sees the compiler); after `trust_revoke`, running that successful build and
  building again are refused.
- **3 and 4**, [`ipc-abuse.e2e.ts`](ipc-abuse.e2e.ts): commands the allowlist does not name
  (Tauri's core and plugin commands, names that only look like ours) are dropped by the isolation
  hook, and the backend's debug log never names them. Every command's valid sample with an extra
  key, a `constructor` or `prototype` key, a missing or wrongly typed request or field, or a
  forged channel is dropped. Documents over 33,554,432 units, program input over 65,536 bytes,
  terminals outside 2–1000 × 1–1000, acknowledgements beyond the safe integers and malformed IDs
  are dropped, while the largest valid values reach the backend. Well-formed forged IDs are
  refused with the matching `unknown…` error, and documents the strict loader refuses with
  `invalidDocument` or `newerFormat`. Messages sent around the isolation frame are refused.
- **5**, [`markup.e2e.ts`](markup.e2e.ts): HTML and script in a project's name, description,
  strings, a variable's value, comments and notes, and in a rejected file's keys, run nothing (the
  canary global `__b2cXss` stays unset) and make no element (nothing carries `data-b2c-xss`). The
  text appears literally in the load failure, the top bar, the block fields, the comment bubble,
  the C++, the console and Problems.
- **6**, [`navigation.e2e.ts`](navigation.e2e.ts): leaving the app by `location`, `assign`,
  `replace`, a link, a link to a new window, `window.open`, a form or a meta refresh, for an
  external site, the loopback address, a local file or the other platform's spelling of the app
  origin, is blocked: the page stays, at the app's address, with one window.
- **7**, [`outside-trust-change.e2e.ts`](outside-trust-change.e2e.ts): a define changed or added
  in the project file by another program, or an edit of `trust.json` (another hash, the record
  removed, the file broken), makes the project restricted at the next reload, and building is
  refused again. In the editor, the external-change dialog's _Reload_ brings the Restricted Mode
  banner back.
- [`helpers.e2e.ts`](helpers.e2e.ts): unit tests of the helpers below (no app).

## How a call's fate is decided

[`lib/ipc.ts`](lib/ipc.ts) sends calls from the page and reports each as _answered_, _refused_
(with the backend's `IpcError` code, or Tauri's own error text), or _dropped_. The isolation hook
drops a message by throwing in its frame, so nothing is sent and the call's promise never
settles: a call counts as dropped when a later `app_info` has been answered and a grace period
has passed. If that control call is not answered, the test fails instead, so a broken IPC can
never pass for a dropped message. Every abuse case ([`lib/abuse.ts`](lib/abuse.ts)) names the
outcome it must have; most are built from the generated valid sample of every command
(`src-tauri/isolation-tests/samples.generated.json`), so a new command gets its cases without a
change here.

Two properties of Tauri 2.12 that the cases rely on, found while writing them:

- **`__proto__` keys never reach the hook.** Tauri's serializer in the editor's frame copies the
  arguments with `copy[key] = value`, which makes a `__proto__` key the copy's prototype, and
  structured cloning into the isolation frame keeps own properties only. What remains is the rest
  of the message, which the hook judges as usual (so a `__proto__` key beside valid arguments
  does not stop the call). The tests check that nothing travels through the prototype and that no
  prototype of the page changes; `constructor` and `prototype` keys stay own keys, and the hook
  drops them.
- **A message can skip the isolation frame.** `__TAURI_INTERNALS__.postMessage` sends a message
  straight to the IPC endpoint. With a JSON body the backend cannot decrypt it and refuses it.
  With an empty body Tauri does not decrypt anything, so a command that takes no argument runs
  (every such command is on the allowlist anyway); every command with arguments is refused, and
  so is every command the capability does not grant. The tests send no argument-free command that
  way, because `app_quit` would end the app.

[`lib/compilers.ts`](lib/compilers.ts) watches for compiler, assembler and linker processes: on
Linux the processes below the app (every 100 ms, by `argv[0]` and `comm`), on Windows `tasklist`
image names (every 250 ms). A sample can miss a very short process, so the tests also check that
the build cache has no build folder, which the backend creates before it starts the compiler.

## Running them

Build the app under test as [the harness README](../../README.md) says, then, on Linux:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 xvfb-run -a pnpm --filter @blocks2cpp/desktop run e2e specs/security
```

Pass the filter without `--`: pnpm 11 forwards a `--` to Vitest, which then ignores the filter and
runs the whole suite. The pull-request `e2e` job leaves these tests to the nightly run
(`--exclude 'specs/security/**'`). A run takes about three and a half minutes on Linux. One spec at a time:

```sh
xvfb-run -a pnpm --filter @blocks2cpp/desktop run e2e specs/security/markup.e2e.ts
```

The project files the tests open are copied into a temporary folder first, so the app never
watches or writes the repository. The fixtures of this suite are in
[`../../fixtures/security`](../../fixtures/security/README.md).

## Adding a case

- A new attack file: add it to `tests/security/projects` with its row; the first spec picks it up.
- A new command: regenerate the IPC files; its sample brings its hook and bypass cases. If it
  takes an ID, add a row to `forgedIdCases`.
- A new limit: add its edges (the largest valid value and one more) to `limitCases`.
