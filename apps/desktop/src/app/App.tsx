import { EditorWorkspace } from '../editor/EditorWorkspace';
import { WindowBanners } from './banners';
import { EditorLayout } from './layout/EditorLayout';
import { StatusBar } from './layout/StatusBar';
import { Toolbar } from './layout/Toolbar';
import { useRegisteredScreen } from './screens';
import { type ScreenId, useAppStore } from './store';

/** The accessible names of the full-window pages. */
const SCREEN_LABELS: Record<ScreenId, string> = {
  start: 'Start page',
  editor: 'Block editor',
  toolchainSetup: 'Set up a C++ compiler',
  settings: 'Settings',
};

/**
 * The main window (docs/spec/04-user-interface.md §4.1): the toolbar, the banners features show
 * under it (such as Restricted Mode, see `banners.tsx`), the editor with its docks, and the status
 * bar. A full-window page (start, toolchain setup, settings) replaces the editor while
 * `ui.screen` names it and a feature provides it; the editor stays mounted underneath, so the
 * workspace and the console keep their state.
 */
export function App() {
  const screen = useAppStore((state) => state.ui.screen);
  const Screen = useRegisteredScreen(screen);

  return (
    <div className="shell">
      <Toolbar />
      <WindowBanners />
      <div className="shell-body">
        <EditorLayout workspace={<EditorWorkspace />} hidden={Screen !== null} />
        {Screen !== null && (
          <main className="screen" aria-label={SCREEN_LABELS[screen]}>
            <Screen />
          </main>
        )}
      </div>
      <StatusBar />
    </div>
  );
}
