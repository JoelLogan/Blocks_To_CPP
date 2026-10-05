# Frontend packages

The desktop app ([`apps/desktop`](../apps/desktop/README.md)) is built from small pnpm workspace
packages, one concern each, as laid out in
[spec §2.3](../docs/spec/02-architecture.md#23-repository-layout). This file describes the
packages, the rules for which may use which, and the template every package follows.

## Packages and layering

| Package                     | Contents                                                                                                                                              | May depend on                                                   |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| `@blocks2cpp/ipc-types`     | TypeScript types and the typed client generated from the Rust IPC commands                                                                            | nothing                                                         |
| `@blocks2cpp/b2c-core-wasm` | The Rust compiler core built for WebAssembly, and its loader                                                                                          | `ipc-types` (types only)                                        |
| `@blocks2cpp/blockly-ext`   | Everything that depends on Blockly: blocks, custom fields, theme, connection checker, mutators ([ADR-0002](../docs/adr/0002-block-editor-blockly.md)) | `blockly`, `ipc-types`, `b2c-core-wasm`                         |
| `@blocks2cpp/catalog-gen`   | Build-time generator: the block catalog to TypeScript block definitions, the toolbox and the block reference                                          | dev dependencies only; nothing depends on it                    |
| `@blocks2cpp/desktop`       | The app ([`apps/desktop`](../apps/desktop/README.md))                                                                                                 | the three runtime packages above (not `catalog-gen`); libraries |

[`tools/check-package-layering.py`](../tools/check-package-layering.py) enforces the last column
in CI (the `frontend` job of [`desktop.yml`](../.github/workflows/desktop.yml)). It also checks the
supply-chain rules of [spec §8.9](../docs/spec/08-security.md#89-supply-chain) that `package.json`
files can break:

- every package is private (nothing here is published);
- every external dependency is pinned to an exact version (`pnpm add` writes `^` ranges: rewrite
  them);
- workspace packages are referenced as `"workspace:*"`, so pnpm never fetches a same-named
  package from the registry, and every `@blocks2cpp/` name is a workspace package;
- no package has a script that pnpm runs by itself: `preinstall`, `install`, `postinstall`,
  `preprepare`, `prepare` and `postprepare` (every install), `pnpm:devPreinstall` (the root, before
  an install), `prepublishOnly` (an injected workspace package) and `prepublish` (`pnpm rebuild`).

`python3 tools/check-package-layering.py --self-test` checks the checker against generated
workspaces that break each rule.

## Package template

A package lives in `packages/<name>/`, which `pnpm-workspace.yaml` already includes. Copy
[`blockly-ext`](blockly-ext/) for the configuration files.

### package.json

```json
{
  "name": "@blocks2cpp/<name>",
  "version": "0.1.0",
  "private": true,
  "description": "What the package does, in one sentence",
  "license": "Apache-2.0",
  "type": "module",
  "exports": {
    ".": "./src/index.ts"
  },
  "scripts": {
    "typecheck": "tsc -p tsconfig.json",
    "lint": "eslint --max-warnings 0 .",
    "format": "prettier --write .",
    "format:check": "prettier --check .",
    "test": "vitest run",
    "test:coverage": "vitest run --coverage",
    "test:watch": "vitest"
  },
  "dependencies": {},
  "devDependencies": {}
}
```

- The version is the app's version (the workspace `Cargo.toml`).
- Packages export their TypeScript source; there is no separate compile step. Vite bundles them
  into the app, and Vitest and `tsc` read them directly.
- Development tools use the same exact versions as `apps/desktop` (TypeScript, ESLint,
  typescript-eslint, Prettier, Vitest, happy-dom), so the lockfile holds one copy of each.
- Every new dependency must install under the policy in `pnpm-workspace.yaml`: published at least
  7 days ago, no weaker publishing provenance than earlier versions (`trustPolicy: no-downgrade`),
  no git or tarball sub-dependencies, and no install scripts. It also needs a row in the dependency
  table of [`apps/desktop/README.md`](../apps/desktop/README.md#dependencies) covering purpose,
  maintenance, licence, size and transitive count.

### tsconfig.json

The compiler options of [spec §9.1](../docs/spec/09-quality-and-delivery.md#91-engineering-standards),
the same as the app's: `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`,
`noImplicitOverride`, `noImplicitReturns`, `noFallthroughCasesInSwitch`,
`noPropertyAccessFromIndexSignature`, `noUnusedLocals`, `noUnusedParameters`,
`verbatimModuleSyntax` and `erasableSyntaxOnly`, with `noEmit`. It includes `src`, `test` and
`vitest.config.ts`.

### ESLint (eslint.config.js)

A flat config with typescript-eslint's `strictTypeChecked` and `stylisticTypeChecked`,
`eslint-plugin-no-unsanitized`, and the security rules of
[spec §8.8](../docs/spec/08-security.md#88-webview-and-ipc-hardening): no `innerHTML`,
`outerHTML`, `insertAdjacentHTML`, `document.write`, `eval`, `new Function`, string arguments to
`setTimeout` or `setInterval`, or `javascript:` URLs. Lint runs with `--max-warnings 0`, so a
warning fails it too. Keep the security rules identical in every package.

Packages with React components also take `eslint-plugin-react-hooks`, `eslint-plugin-jsx-a11y`
(the strict set) and the ban on `dangerouslySetInnerHTML`, as `apps/desktop/eslint.config.js`
does. `eslint-plugin-react` is not used: its dependency `semver@6.3.1` fails the trust policy, and
the one rule the security rules need from it (`react/no-danger`) is a `no-restricted-syntax` rule
instead.

### Prettier

`.prettierrc.json` with `singleQuote: true` and `printWidth: 100`, and a `.prettierignore` that
lists `coverage/` (and any build output).

### Tests (vitest.config.ts)

- Vitest with the `happy-dom` environment (a DOM without layout; packages without DOM code may use
  `node`), test files in `src/**/*.test.ts` or `test/**/*.test.ts`.
- Coverage with the `v8` provider over `src/**` (without tests, `.d.ts` files and generated code),
  the reporters `text`, `lcov` and `json-summary` into `coverage/`, and the gate
  `thresholds: { lines: 75 }` ([spec §9.2](../docs/spec/09-quality-and-delivery.md#92-testing-strategy)).
- Blockly runs in happy-dom, headless and injected with the Zelos renderer, once the setup file
  installs the shim in `test/dom-shims.ts` (Blockly 12 calls `document.addEventListener` without a
  receiver, which browsers allow and happy-dom does not).
- Components are checked with axe-core: `expectNoAxeViolations(container)` in
  [`apps/desktop/src/test/axe.ts`](../apps/desktop/src/test/axe.ts) fails a test on any WCAG 2.2 AA
  problem. A package that renders UI adds `axe-core` as a dev dependency and the same helper.
  Rules that need layout (colour contrast, target size) are left to the end-to-end tests.

## Checks

From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm desktop:check                       # typecheck, lint and format:check in every package
pnpm test:web                            # the tests of every package
pnpm -r --if-present run test:coverage   # with each package's coverage gate, as CI runs them
python3 tools/check-package-layering.py
```

CI runs them in [`desktop.yml`](../.github/workflows/desktop.yml): the `frontend` job (typecheck,
lint, format and layering) and the `test-web` job (the WebAssembly core build, then the tests with
coverage; it writes a summary to the job page and keeps the lcov reports as an artifact).

## Adding a package

1. Create `packages/<name>/` from the template above.
2. Add its rules to `ALLOWED_WORKSPACE_DEPS` and `ALLOWED_RUNTIME_EXTERNALS` in
   `tools/check-package-layering.py`, and a row to the table above. A new layering rule also
   belongs in [spec §2.3](../docs/spec/02-architecture.md#23-repository-layout).
3. Run `pnpm install` and commit the updated `pnpm-lock.yaml`.

## Security

User content (block text, comments, compiler output and program output) is only ever rendered as
text: React text nodes, SVG text in Blockly fields, or terminal cells. Nothing is loaded from the
network, and every library must work under the app's Content Security Policy and with
`freezePrototype: true` ([spec §8.8](../docs/spec/08-security.md#88-webview-and-ipc-hardening)).
