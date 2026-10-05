import { useSyncExternalStore } from 'react';

import { App } from './App';
import type { AppRuntime } from './bootstrap';
import { DialogHost } from './dialogs';
import { StartingScreen, StartupErrorScreen } from './StartupScreens';
import { HintProvider } from './ui/Hint';

/**
 * The root of the window: the start-up screens until the runtime is ready, then the main window,
 * plus the providers and the dialog host every part of it uses.
 */
export function Root({ runtime }: { runtime: AppRuntime }) {
  const phase = useSyncExternalStore(runtime.subscribe, runtime.getPhase);

  let content;
  switch (phase.kind) {
    case 'starting':
      content = <StartingScreen />;
      break;
    case 'ready':
      content = <App />;
      break;
    case 'versionMismatch':
    case 'failed':
      content = <StartupErrorScreen phase={phase} />;
      break;
  }

  return (
    <HintProvider>
      {content}
      <DialogHost queue={runtime.dialogs} />
    </HintProvider>
  );
}
