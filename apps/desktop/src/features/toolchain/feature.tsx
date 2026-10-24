import type { Feature } from '../../app/features';
import { ToolchainController } from './controller';
import { ToolchainPage } from './ToolchainPage';

/**
 * The toolchain feature (docs/spec/04-user-interface.md §4.6, 07 §7.2): the `toolchainSetup`
 * screen, which is the setup page while no compiler can build and the toolchain list otherwise.
 *
 * When it is installed it reads the toolchain list (the cached results, shown at once) and the
 * setup information, and it follows `toolchainsUpdated`. When discovery ends without a usable
 * compiler it shows the page, once until a usable compiler appears again. The status bar's
 * toolchain item and a held-back Run or Build open the page too (the shell does that once the
 * screen is registered).
 */
export const toolchainFeature: Feature = (ctx) => {
  const controller = new ToolchainController(ctx);
  function ToolchainSetupScreen() {
    return <ToolchainPage controller={controller} />;
  }
  const removeScreen = ctx.screens.registerScreen('toolchainSetup', ToolchainSetupScreen);
  controller.start();
  return () => {
    controller.dispose();
    removeScreen();
  };
};
