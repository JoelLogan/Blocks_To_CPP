/** Runs before every test file (vitest.config.ts `setupFiles`). */
import { installEventTargetReceiverShim } from './dom-shims';

// Without it, `Blockly.inject` fails in happy-dom (see dom-shims.ts).
installEventTargetReceiverShim();
