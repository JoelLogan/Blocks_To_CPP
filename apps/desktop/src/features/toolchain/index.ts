/**
 * The toolchain feature: the setup page for people without a compiler and the toolchain list
 * (docs/spec/04-user-interface.md §4.6). Install `toolchainFeature` with the other features
 * (src/features/index.ts).
 */
export { toolchainFeature } from './feature';
export {
  needsSetup,
  type SetupLink,
  type ToolchainAction,
  ToolchainController,
  type ToolchainOutcome,
  type ToolchainPageState,
} from './controller';
export {
  INSTALL_COMMANDS,
  instructionPlatforms,
  LINUX_PACKAGE_MANAGERS,
  linuxCommands,
  linuxPackageManager,
  type LinuxPackageManager,
} from './instructions';
export { REASONS, reasonFor, type ReasonSummary } from './reasons';
export { failureText, LINK_ADDRESSES, pageTitle, ToolchainPage } from './ToolchainPage';
