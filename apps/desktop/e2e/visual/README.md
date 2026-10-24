# Visual diff of the block canvas and the chrome

The visual diff of [09 §9.2](../../../../docs/spec/09-quality-and-delivery.md#92-testing-strategy):
the Zelos renderer and the Blocks2Cpp theme as WebKitGTK and WebView2 draw them
([10 §10.2](../../../../docs/spec/10-roadmap.md), risk 2), and the window's chrome around them (the
M2 polish plan's W7-1). It runs in the `e2e` job of
[`desktop.yml`](../../../../.github/workflows/desktop.yml) on every pull request, after the other
end-to-end tests, and nightly ([`nightly.yml`](../../../../.github/workflows/nightly.yml)).

## The block canvas

[`canvas.visual.e2e.ts`](canvas.visual.e2e.ts) opens the guessing game
([`fixture.ts`](fixture.ts): `examples/guessing_game.b2c` with its module's view set to zoom 1.0)
and takes a WebDriver screenshot of the workspace element (the toolbox, its flyout and the canvas):

- the window is 1000×700, set through the app's window-size seam (`B2C_E2E_WINDOW_SIZE`); on a
  screen too small for it the app keeps the window on the screen, and a warning says so;
- the C++ panel and the bottom panel are folded away, the _Program_ category is chosen in the
  toolbox, the pointer rests on the status bar, nothing has focus and the text caret is hidden;
- the light theme (the test fails when the system prefers dark);
- the page has loaded its fonts, runs no animation and has drawn 20 frames in a row on time.

## The chrome

[`chrome.visual.e2e.ts`](chrome.visual.e2e.ts) takes the scenes of [`scenes.ts`](scenes.ts) in
one launch of the app per theme, in the smallest window the app allows (800×560, through the
window-size seam), with a new project from the Empty template whose program prints three lines.
Each scene is cut out of a screenshot of the whole window:

| Scene                         | Area                       | Shows                                                                                                                          |
| ----------------------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `chrome-window-smallest`      | the window                 | the editor: the program on the canvas, its C++, the console not run yet                                                        |
| `chrome-toolbar-select`       | the toolbar                | the Debug/Release drop-down closed, nothing hovered                                                                            |
| `chrome-toolbar-select-hover` | the toolbar                | the pointer on the drop-down (its hover colour)                                                                                |
| `chrome-menu-open`            | the window                 | the ≡ main menu open over the editor and its toolbox                                                                           |
| `chrome-toolbox-<category>`   | the toolbox and its flyout | one per category (`program`, `variables`, `math`, `logic`, `text`, `control`, `loops`, `io`, `functions`), after a click on it |
| `chrome-console-scrolled`     | the bottom panel           | after two runs, scrolled up 4 lines from the end (the end of the first run and _New run_), at the panel's first height         |
| `chrome-console-two-runs`     | the bottom panel           | after two runs, the panel at its tallest: the first run, _New run_, the second run                                             |

The themes: light on both systems, and on Linux dark too, as separate scenes with their own
baselines (`…-dark`): the app is started with `GTK_THEME=Adwaita:dark`, which WebKitGTK reports
as `prefers-color-scheme: dark` (the light launch sets `GTK_THEME=Adwaita`, so a dark GTK theme
of the desktop does not carry over). WebView2 follows Windows' own setting, which a CI runner
cannot change for one app; the dark scenes on Windows come with the Appearance setting (M3). Each
scene checks that the page prefers the theme it is taken in, so a desktop that asks GTK for dark
applications fails the light scenes instead of making wrong candidates.

What makes a scene the same from run to run:

- **Window and fonts:** the fixed window size; the page's fonts have loaded.
- **Toolbox:** before the whole-window scenes (and the canvas screenshot) the _Program_ category is
  chosen with a click, and must then be the only row drawn selected; each toolbox scene clicks its
  own category. Which row is highlighted is part of the picture, and is never left to what came
  before.
- **At rest:** no Web Animation (CSS animation or transition) runs, the page has drawn 20 frames
  in a row on time ([`settle.ts`](settle.ts)), and two screenshots 150 ms apart give the same
  pixels ([`capture.ts`](capture.ts)); a scene that still changes after six fails as not taken,
  with its last image as the candidate and a `.diff.png` of what moved. Each wait for the page
  (its fonts, its animations, its frames) ends after 20 s, and the page then says what still
  moves, well before WebDriver's own 30 s script timeout. A page that does not come to rest stops
  the test at once, with the scenes after it recorded as not taken: each would wait as long.
- **Pointer, focus, caret:** the pointer is parked in the status bar's padding (bottom-left
  corner), where it hovers nothing, except in the hover scene; nothing has the focus (no focus
  ring, the console's cursor drawn as the steady outline of an unfocused terminal) and the text
  caret is transparent.
- **Scroll bars:** WebKitGTK's overlay scroll bars appear when the pointer passes over a scrolling
  area or it scrolls, and fade out over the next four seconds or so, by a timer the page cannot
  see; they take no room, so they are hidden (`scrollbar-width: none`) before each screenshot.
  Classic scroll bars, which take room (WebView2's), are left as they are.
- **Masks:** what changes from machine to machine or from run to run is painted over in magenta,
  in the candidate as in the baseline: the status bar's toolchain (the g++ version and flavour),
  the console's elapsed time, and the C++ panel's first line (`// Generated by Blocks2Cpp
<version> …`, the app's version). The toolchain and the elapsed time are given a fixed width
  first, so a longer or shorter text does not move what follows them.
- **The program:** three fixed lines, no input, no time or path; the console's header shows the
  state (`Finished (exit code 0)`) and the notices, which are the machine's (_Process group only_
  where Linux has no cgroup for the run) and the same on every run there.

Every scene is compared on both systems. On Windows the console scenes have not been checked yet
(no Windows machine here): the pseudoconsole repaints each run from the top-left corner after
blank lines (04 §4.5), so there the second run's output starts at the top of the view. If one
turns out not to come to rest on WebView2, it gets `systems: ['linux']` in `scenes.ts` and a line
here saying why.

A scene that fails does not stop the others ([`scene-run.ts`](scene-run.ts)): the test fails at
the end with all the failures, and the summary lists every scene, the ones the test never reached
as not taken. The canvas screenshot is recorded as not taken, too, when its test stops before it.

## Comparing

[`compare.ts`](compare.ts) compares a screenshot with this system's baseline,
`baselines/<linux|windows>/<name>.png`: pixelmatch with a colour threshold of 0.1 counts the
differing pixels (anti-aliased edges are not counted), and more than 0.5% of them fails. A
screenshot of another size than its baseline fails too. The threshold is pixelmatch 8's perceptual
colour distance (OKLab HyAB, where 1 is black against white): small rendering noise is not counted,
and [`compare.test.ts`](compare.test.ts) checks that on every committed baseline. [`baselines.ts`](baselines.ts)
(`checkShot`) does it for every screenshot, the canvas and each chrome scene alike.

## Baselines

Each system has its own baselines, because fonts and anti-aliasing differ between WebKitGTK and
WebView2 and between machines. They must come from the CI runners, never from a developer's
machine. Every run writes each screenshot as a candidate, with a `.diff.png` that marks the
differing pixels in red when there is a baseline, into `visual/<system>/` of the artifacts folder;
the CI job uploads them as the artifact `visual-candidates-<os>` (`nightly-visual-candidates-<os>`
nightly), and adds the table of `visual/visual-diff.md` to its summary.

- **No baseline yet** (the first runs on a system, and every new scene): the screenshot passes
  with a warning, and the candidate is the baseline to commit.
- **A deliberate change** (the theme, the renderer, a Blockly update, the chrome): start
  `desktop.yml` by hand with _visual-baselines_ checked. Its e2e job then only writes candidates
  (`B2C_VISUAL_UPDATE=1`) and compares nothing. Review the images in `visual-candidates-<os>` and
  commit them under `baselines/linux/` and `baselines/windows/` with the names they have there
  (`canvas-guessing-game.png`, `chrome-<scene>.png` and, on Linux, `chrome-<scene>-dark.png`).

## Running it locally (Linux)

With the app under test, `tauri-driver`, `WebKitWebDriver` and `xvfb`, as for the end-to-end tests
(see [`../README.md`](../README.md)):

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 xvfb-run -a \
  pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/visual/vitest.visual.config.ts --project visual
```

It takes about a minute: three launches (the canvas, the light chrome, the dark chrome), each
chrome launch building and running its program twice. A local screenshot is only comparable with
a baseline made on the same machine. The comparison, the masks and cut-outs, the screenshots
taken until two are the same, the scene list and its bookkeeping (every scene in the summary once,
whatever stopped the test), the page scripts and their time limits, the fixture and the summary
are tested without the app:

```sh
pnpm --filter @blocks2cpp/desktop exec vitest run --config e2e/visual/vitest.visual.config.ts --project visual-unit
```

The run's global set-up ([`global.ts`](global.ts)) starts a new visual summary, and at the end
writes the Trusted Types summary of [08 §8.8](../../../../docs/spec/08-security.md#88-webview-and-ipc-hardening)
and the long-task summary again from their reports, without clearing them: the CI job runs the
visual diff after the other end-to-end tests, and its summary shows the counts of all of them.
