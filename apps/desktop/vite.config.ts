import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vite';

import { BLOCKLY_MEDIA_DIR } from './src/editor/media.ts';

/** Blockly's images and cursors. Its sounds are left out: the workspace has sound turned off. */
const BLOCKLY_MEDIA_FILE = /\.(?:cur|gif|png|svg)$/;

/**
 * Copies Blockly's media files into the build, so that the app loads nothing from the network
 * (Blockly's default media path is a web server). The dev server serves them from node_modules.
 */
function blocklyMedia(): Plugin {
  let mediaDir = '';
  return {
    name: 'blocks2cpp:blockly-media',
    apply: 'build',
    configResolved(config) {
      mediaDir = `${config.root}/node_modules/blockly/media`;
    },
    async generateBundle() {
      for (const name of await this.fs.readdir(mediaDir)) {
        if (BLOCKLY_MEDIA_FILE.test(name)) {
          this.emitFile({
            type: 'asset',
            fileName: `${BLOCKLY_MEDIA_DIR}/${name}`,
            source: await this.fs.readFile(`${mediaDir}/${name}`),
          });
        }
      }
    },
  };
}

// https://vite.dev/config/ and https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react(), blocklyMedia()],
  // Keep Rust compiler errors from `tauri dev` visible.
  clearScreen: false,
  server: {
    // tauri.conf.json's devUrl expects exactly this port.
    port: 1420,
    strictPort: true,
    host: 'localhost',
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: {
    // WebView2 on Windows (Chromium) and WebKitGTK 2.40+ on Linux.
    target: ['es2022', 'chrome110', 'safari16'],
    // Every asset is a file of its own: nothing is inlined as a data: URL.
    assetsInlineLimit: 0,
    // No source maps in the shipped app.
    sourcemap: false,
    // The app loads its bundle from disk, so one large chunk costs nothing; Blockly alone is
    // about 700 kB.
    chunkSizeWarningLimit: 2048,
  },
});
