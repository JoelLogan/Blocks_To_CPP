import { useEffect, useState } from 'react';

import { BlocklyWorkspace } from '../editor/BlocklyWorkspace';
import { appVersion } from '../lib/ipc';
import { PanelPlaceholder } from '../panels/PanelPlaceholder';

/** The bottom dock's tabs (docs/spec/04-user-interface.md §4.1); not interactive yet. */
const DOCK_TABS = ['Console', 'Problems', 'Build output'] as const;

/** The app's version from the backend, or `undefined` outside the desktop app. */
function useAppVersion(): string | undefined {
  const [version, setVersion] = useState<string>();
  useEffect(() => {
    let current = true;
    appVersion().then(
      (value) => {
        if (current) {
          setVersion(value);
        }
      },
      () => {
        // Not running inside the desktop app (e.g. `pnpm dev` in a browser): show no version.
      },
    );
    return () => {
      current = false;
    };
  }, []);
  return version;
}

/**
 * The main window (docs/spec/04-user-interface.md §4.1). Milestone M0 is the shell:
 * the top bar, an empty block canvas, and placeholders for the toolbox, the C++ panel
 * and the bottom dock.
 */
export function App() {
  const version = useAppVersion();

  return (
    <div className="shell">
      <header className="toolbar">
        <span className="app-name">Blocks2Cpp</span>
      </header>

      <nav className="toolbox-panel" aria-label="Block categories">
        <PanelPlaceholder title="Blocks">
          The block categories arrive with the editor.
        </PanelPlaceholder>
      </nav>

      <main className="workspace" aria-label="Block workspace">
        <BlocklyWorkspace />
      </main>

      <aside className="code-panel" aria-label="Generated C++">
        <PanelPlaceholder title="C++">The C++ for your blocks will appear here.</PanelPlaceholder>
      </aside>

      <section className="dock" aria-label="Console, problems and build output">
        <div className="dock-tabs">
          {DOCK_TABS.map((tab) => (
            <span key={tab} className="dock-tab">
              {tab}
            </span>
          ))}
        </div>
        <p className="placeholder-text">Your program&apos;s output will appear here.</p>
      </section>

      <footer className="status-bar">
        <span>{version === undefined ? 'Blocks2Cpp' : `Blocks2Cpp ${version}`}</span>
      </footer>
    </div>
  );
}
