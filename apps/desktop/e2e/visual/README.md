# Visual diff of the block canvas

The visual diff of [09 §9.2](../../../../docs/spec/09-quality-and-delivery.md#92-testing-strategy):
the Zelos renderer and the Blocks2Cpp theme as WebKitGTK and WebView2 draw them
([10 §10.2](../../../../docs/spec/10-roadmap.md), risk 2). It runs in the `e2e` job of
[`desktop.yml`](../../../../.github/workflows/desktop.yml) on every pull request, after the other
end-to-end tests, and nightly ([`nightly.yml`](../../../../.github/workflows/nightly.yml)).

[`canvas.visual.e2e.ts`](canvas.visual.e2e.ts) opens the guessing game
([`fixture.ts`](fixture.ts): `examples/guessing_game.b2c` with its module's view set to zoom 1.0)
and takes a WebDriver screenshot of the workspace element (the toolbox, its flyout and the canvas):

- the window is 1000×700 (a native driver that cannot resize the app's window leaves it at the
  app's own size, which is as repeatable; a warning says so);
- the C++ panel and the bottom panel are folded away, the pointer rests on the status bar, nothing
  has focus and the text caret is hidden;
- the light theme (the test fails when the system prefers dark);
- the page has loaded its fonts and has drawn 20 frames in a row on time.

[`compare.ts`](compare.ts) compares the screenshot with this system's baseline,
`baselines/<linux|windows>/canvas-guessing-game.png`: pixelmatch with a colour threshold of 0.1
counts the differing pixels (anti-aliased edges are not counted), and more than 0.5% of them fails.
A screenshot of another size than its baseline fails too.

## Baselines

Each system has its own baselines, because fonts and anti-aliasing differ between WebKitGTK and
WebView2 and between machines. They must come from the CI runners, never from a developer's
machine. Every run writes its screenshot as a candidate, with a `.diff.png` that marks the differing
pixels in red when there is a baseline, into `visual/<system>/` of the artifacts folder; the CI job
uploads them as the artifact `visual-candidates-<os>`, and adds the table of
`visual/visual-diff.md` to its summary.

- **No baseline yet** (the first runs on a system): the test passes with a warning, and the
  candidate is the baseline to commit.
- **A deliberate change of the canvas** (the theme, the renderer, a Blockly update): start
  `desktop.yml` by hand with _visual-baselines_ checked. Its e2e job then only writes candidates
  (`B2C_VISUAL_UPDATE=1`) and compares nothing. Review the images in `visual-candidates-<os>` and
  commit them as `baselines/linux/canvas-guessing-game.png` and
  `baselines/windows/canvas-guessing-game.png`.

## Running it locally (Linux)

With the app under test, `tauri-driver`, `WebKitWebDriver` and `xvfb`, as for the end-to-end tests
(see [`../README.md`](../README.md)):

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 xvfb-run -a \
  pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/visual/vitest.visual.config.ts --project visual
```

A local screenshot is only comparable with a baseline made on the same machine. The comparison, the
fixture and the summary are tested without the app:

```sh
pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/visual/vitest.visual.config.ts --project visual-unit
```

The run's global set-up ([`global.ts`](global.ts)) starts a new visual summary, and at the end
writes the Trusted Types summary of [08 §8.8](../../../../docs/spec/08-security.md#88-webview-and-ipc-hardening)
again from the report, without clearing the report: the CI job runs the visual diff after the other
end-to-end tests, and its summary shows the counts of all of them.
