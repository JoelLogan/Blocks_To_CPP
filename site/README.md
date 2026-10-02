# Specification website

This folder builds the Blocks2Cpp specification website: a single HTML page
generated from the Markdown in [`docs/spec/`](../docs/spec/README.md) and
[`docs/adr/`](../docs/adr/README.md), published with GitHub Pages.

The Markdown files are the single source of truth. Edit them, not the
generated page.

## Build locally

Requirements: Node.js 22+ and pnpm (the version is pinned in the root
`package.json`; [Corepack](https://nodejs.org/api/corepack.html) or the pnpm
installer will pick it up).

```sh
pnpm install --frozen-lockfile
pnpm site:build          # writes site/dist/index.html and site/dist/assets/
```

Open `site/dist/index.html` in a browser, or serve the folder (for example
`python3 -m http.server --directory site/dist`).

## What the build checks

The build fails, listing every problem at once, when the docs contain:

* a link to a file that does not exist, or to a heading that does not exist
* a link that is not `https://` (other than links to files in the repository)
* raw HTML or images (the page renders Markdown only)
* duplicate heading anchors or a document without exactly one `#` title

Links between documents become in-page anchors (for example
`03-block-language.md#34-expression-slots` → `#ch03-34-expression-slots`).
Links to other repository files point to GitHub.

## Publishing

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) builds the site
on every push and pull request that touches the docs or the site, and deploys
it from the repository's default branch.

One-time setup by a repository admin: **Settings → Pages → Build and
deployment → Source: GitHub Actions**. After the next push to the default
branch (or a manual run of the *Pages* workflow), the site is live at
`https://joellogan.github.io/Blocks_To_CPP/`.

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
| `src/fonts/` | Atkinson Hyperlegible Next (SIL Open Font License 1.1, see `OFL.txt`) |
