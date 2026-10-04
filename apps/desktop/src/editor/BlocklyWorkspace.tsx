import * as Blockly from 'blockly/core';
import * as English from 'blockly/msg/en';
import { useEffect, useRef } from 'react';

import { BLOCKLY_MEDIA_DIR } from './media';

// Blockly's user-interface text (context menus, tooltips). The UI language
// becomes selectable with the i18n work (docs/spec/04-user-interface.md §4.9).
Blockly.setLocale(English as unknown as Record<string, string>);

/**
 * Where Blockly loads its images and cursors from: the dev server serves them
 * straight from the package, the build copies them (see vite.config.ts).
 * Never Blockly's default, which is a web server.
 */
const MEDIA_PATH = import.meta.env.DEV ? '/node_modules/blockly/media/' : `/${BLOCKLY_MEDIA_DIR}/`;

/** Workspace colours; they match the `--b2c-canvas*` tokens in app.css. */
const lightTheme = Blockly.Theme.defineTheme('b2c-light', {
  name: 'b2c-light',
  base: Blockly.Themes.Zelos,
  componentStyles: {
    workspaceBackgroundColour: '#f7f8fb',
    scrollbarColour: '#c5cbd8',
  },
});

const darkTheme = Blockly.Theme.defineTheme('b2c-dark', {
  name: 'b2c-dark',
  base: Blockly.Themes.Zelos,
  componentStyles: {
    workspaceBackgroundColour: '#191d26',
    scrollbarColour: '#4a5263',
    flyoutBackgroundColour: '#232836',
    flyoutForegroundColour: '#e6e9f0',
  },
});

const darkScheme = window.matchMedia('(prefers-color-scheme: dark)');

function currentTheme(): Blockly.Theme {
  return darkScheme.matches ? darkTheme : lightTheme;
}

/**
 * The Blockly workspace with the Scratch-style Zelos renderer. Milestone M0
 * shows an empty canvas: no toolbox and no blocks yet.
 */
export function BlocklyWorkspace() {
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = host.current;
    if (element === null) {
      return;
    }
    const workspace = Blockly.inject(element, {
      renderer: 'zelos',
      theme: currentTheme(),
      media: MEDIA_PATH,
      sounds: false,
      trashcan: true,
      grid: { spacing: 24, length: 2, colour: '#8f98ab55', snap: true },
      move: { scrollbars: true, drag: true, wheel: true },
      zoom: {
        controls: true,
        wheel: false,
        pinch: true,
        startScale: 0.9,
        maxScale: 3,
        minScale: 0.3,
      },
    });

    // Blockly sizes its SVG once; follow the panel when the window or a dock resizes.
    const resizeObserver = new ResizeObserver(() => {
      Blockly.svgResize(workspace);
    });
    resizeObserver.observe(element);

    const onSchemeChange = () => {
      workspace.setTheme(currentTheme());
    };
    darkScheme.addEventListener('change', onSchemeChange);

    return () => {
      darkScheme.removeEventListener('change', onSchemeChange);
      resizeObserver.disconnect();
      workspace.dispose();
    };
  }, []);

  return <div ref={host} className="blockly-host" />;
}
