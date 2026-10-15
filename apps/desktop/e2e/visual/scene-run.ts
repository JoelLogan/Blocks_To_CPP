/**
 * The bookkeeping of one launch's chrome scenes (chrome.visual.e2e.ts): each scene is taken by the
 * function the test gives (the page brought to rest, the scene cut out and masked), compared with
 * this system's baseline (baselines.ts `checkShot`), and recorded in the summary, so that the
 * summary lists every scene of this system once per theme, the ones the test never reached as not
 * taken. A scene that fails does not stop the others.
 */
import { checkShot, recordOutcome, type VisualSettings, writeCandidate } from './baselines';
import { UnstableSceneError } from './capture';
import { compareScreenshots } from './compare';
import { CHROME_SCENES, type ChromeScene, chromeShotName, sceneRunsOn, type Theme } from './scenes';
import { warn } from './settle';

/** Takes a scene's image: the page brought to rest, the scene cut out and masked. */
export type ShootScene = (scene: ChromeScene) => Promise<Buffer>;

/** The text of an error, for the summary. */
function reasonOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The scenes of one launch: takes them, and records what each came to. */
export class SceneRun {
  readonly #settings: VisualSettings;
  readonly #theme: Theme;
  readonly #shoot: ShootScene;
  readonly #scenes: readonly ChromeScene[];
  /** The screenshot names recorded in the summary so far. */
  readonly #recorded = new Set<string>();
  /** Why scenes failed, one line each. */
  readonly failures: string[] = [];

  constructor(
    settings: VisualSettings,
    theme: Theme,
    shoot: ShootScene,
    scenes: readonly ChromeScene[] = CHROME_SCENES,
  ) {
    this.#settings = settings;
    this.#theme = theme;
    this.#shoot = shoot;
    this.#scenes = scenes;
  }

  /** The scenes of this system, with their screenshot names in this theme. */
  #ours(): { scene: ChromeScene; name: string }[] {
    return this.#scenes
      .filter((scene) => sceneRunsOn(scene, this.#settings.os))
      .map((scene) => ({ scene, name: chromeShotName(scene, this.#theme) }));
  }

  /**
   * Takes scene `id` and compares it. A failure is noted (and recorded in the summary) and does
   * not stop the test.
   */
  async take(id: string): Promise<void> {
    const scene = this.#scenes.find((candidate) => candidate.id === id);
    if (scene === undefined) {
      throw new Error(`There is no chrome scene ${id}`);
    }
    if (!sceneRunsOn(scene, this.#settings.os)) {
      return;
    }
    const name = chromeShotName(scene, this.#theme);
    let image: Buffer;
    try {
      image = await this.#shoot(scene);
    } catch (error: unknown) {
      if (error instanceof UnstableSceneError && error.changed !== null) {
        // What moved: the last image, and the pixels that differ from the one before in red.
        writeCandidate(
          this.#settings,
          name,
          error.last,
          compareScreenshots(error.previous, error.last).diffPng,
        );
      }
      this.notTaken(name, error);
      return;
    }
    const check = checkShot(this.#settings, name, image);
    this.#recorded.add(name);
    if (check.warning !== null) {
      warn(check.warning);
    }
    if (check.failure !== null) {
      this.failures.push(check.failure);
    }
  }

  /** Records `name` as not taken, because of `error`. */
  notTaken(name: string, error: unknown): void {
    const reason = reasonOf(error);
    recordOutcome(this.#settings, name, { kind: 'notTaken', reason });
    this.#recorded.add(name);
    this.failures.push(`${name} was not taken: ${reason}`);
  }

  /** Records every scene of this system not recorded yet as not taken, because of `error`. */
  notReached(error: unknown): void {
    const reason = `the test stopped before it: ${reasonOf(error)}`;
    for (const { name } of this.#ours()) {
      if (!this.#recorded.has(name)) {
        this.notTaken(name, new Error(reason));
      }
    }
  }

  /** The scenes of this system that were never recorded (the test's own check). */
  missing(): string[] {
    return this.#ours()
      .map(({ name }) => name)
      .filter((name) => !this.#recorded.has(name));
  }
}
