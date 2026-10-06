# Visual diff baselines

One folder per system, `linux/` and `windows/`, each holding `canvas-guessing-game.png`: the block
canvas as the CI runner of that system draws it (see [`../README.md`](../README.md)). Take them from
the `visual-candidates-<os>` artifact of a CI run, after looking at them; never from a developer's
machine. Until a system has its baseline, the visual diff passes there with a warning.
