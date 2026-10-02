# Specification website

This folder builds the Blocks2Cpp specification website: a single HTML page
generated from the Markdown in [`docs/spec/`](../docs/spec/README.md) and
[`docs/adr/`](../docs/adr/README.md), published with GitHub Pages.

The Markdown files are the single source of truth. Edit them, not the
generated page.

## Build locally

Requirements: Node.js 22.13 or later, and pnpm. The exact pnpm version is
pinned in the root `package.json`. Run `corepack enable` once to have
[Corepack](https://nodejs.org/api/corepack.html) provide it, or install pnpm 11
yourself.

```sh
pnpm install --frozen-lockfile
pnpm site:build          # writes site/dist/index.html and site/dist/assets/
```

Open `site/dist/index.html` in a browser, or serve the folder (for example
`python3 -m http.server --directory site/dist`).

## What the build checks

The build fails, listing every problem at once, when:

* a link points to a file that does not exist, or to a heading that does not
  exist (in the page's documents, or in any other Markdown file in the
  repository; fragments on non-Markdown files, such as `LICENSE#L5`, are left
  to GitHub)
* a link is not `https://` (other than links to files in the repository)
* a Markdown file or subfolder in `docs/spec/` or `docs/adr/` would be left off
  the page (chapters must be named `NN-name.md` and decisions `NNNN-name.md`)
* a document contains raw HTML or images (the page renders Markdown only), or a
  heading uses a named HTML entity other than `&amp;`, `&lt;`, `&gt;`, `&quot;` or
  `&apos;` (numeric references such as `&#169;` are fine)
* heading anchors are duplicated, or a document does not have exactly one `#`
  title

Heading anchors follow GitHub's rules, so the same `file.md#anchor` link works
on github.com and on the site.

Links between documents become in-page anchors (for example
`03-block-language.md#34-expression-slots` → `#ch03-34-expression-slots`).
Links to other repository files point to GitHub.

## Publishing

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) builds the site
on every push and pull request that touches the docs or the site, and deploys
it from the repository's default branch.

One-time setup by a repository admin:

1. **Settings → Pages → Build and deployment → Source: GitHub Actions.**
2. **Actions → Pages → Run workflow** on the default branch.

The site is then live at `https://joellogan.github.io/Blocks_To_CPP/`. After
that, every push to the default branch that changes the docs, the site or its
build configuration redeploys it. Pushes that change only other files do not
run the workflow.

## Security

The page follows the same rules as the app ([spec §8.8](../docs/spec/08-security.md#88-webview-and-ipc-hardening)):

* A strict Content Security Policy: only same-origin scripts, styles, fonts
  and images, with Trusted Types enforced. There are no inline scripts or
  styles.
* No third-party requests: the font is self-hosted and there are no analytics
  or CDNs.
* Markdown is rendered at build time, and raw HTML is rejected.
* The one npm dependency (`marked`, which has no dependencies of its own) is
  pinned exactly and installed under the workspace's pnpm supply-chain policy
  ([`pnpm-workspace.yaml`](../pnpm-workspace.yaml)).

## Files

| Path | Purpose |
|------|---------|
| `build.mjs` | Generator: discovers documents, renders Markdown, rewrites and validates links, builds the table of contents |
| `src/index.html` | Page template (header, hero, layout, footer) |
| `src/site.css` | Styles and colour tokens for light and dark themes |
| `src/site.js` | Optional enhancements: theme button, current-section highlight, copy buttons |
| `src/theme-init.js` | Applies a saved theme before first paint |
| `src/fonts/` | Atkinson Hyperlegible Next, see below |

## Font

The page uses [Atkinson Hyperlegible Next](https://github.com/googlefonts/atkinson-hyperlegible-next)
version 2.001, © The Atkinson Hyperlegible Next Project Authors, licensed under
the SIL Open Font License 1.1 ([`src/fonts/OFL.txt`](src/fonts/OFL.txt)). This is
the one part of the repository that is not under Apache-2.0.

The files are the Latin-subset variable fonts (weights 200–800) from the
Fontsource package `@fontsource-variable/atkinson-hyperlegible-next@5.3.0`,
copied unchanged. They are not managed by Dependabot; update them by hand.

| File | SHA-256 |
|------|---------|
| `atkinson-hyperlegible-next-latin-wght-normal.woff2` | `18b2a1a39a2fa298b0ba5390aca68462669826c90925656f1c1f6796e0e1bbaf` |
| `atkinson-hyperlegible-next-latin-wght-italic.woff2` | `4a5037bfaf6680f40147407407ec09fa42925774bde809a579283d27f9f08106` |
