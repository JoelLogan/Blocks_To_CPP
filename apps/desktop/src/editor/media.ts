/**
 * The folder of the built app that holds Blockly's images and cursors. vite.config.ts copies
 * them there, and the editor points Blockly at it. Plain data, so the build config can import it.
 */
export const BLOCKLY_MEDIA_DIR = 'blockly-media';
