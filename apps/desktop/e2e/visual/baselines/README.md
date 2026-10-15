# Visual diff baselines

One folder per system, `linux/` and `windows/`, each holding `canvas-guessing-game.png` (the block
canvas) and a `chrome-<scene>.png` for each chrome scene, and on Linux a `chrome-<scene>-dark.png`
for each in the dark theme: the app as the CI runner of that system draws it (see
[`../README.md`](../README.md), which lists the scenes). Take them from the
`visual-candidates-<os>` artifact of a CI run, after looking at them; never from a developer's
machine. Until a screenshot has its baseline, the visual diff passes it with a warning.
