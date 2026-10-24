/**
 * Registering the Blocks2Cpp blocks with Blockly, and installing the editor services into
 * blockly-ext: the fields' symbol and type services and dialogs, the mutators' hooks and the
 * connection checker's type oracle (wave-2 seams of blockly-ext).
 */
import {
  type DialogService as FieldDialogService,
  type EditorServices,
  exprShadowState,
  type InputShadowFactory,
  configureMutators,
  installIdGenerator,
  newId,
  registerB2cBlocks,
  registerB2cMutators,
  resetEditorServices,
  setCheckerTypeOracle,
  setEditorServices,
  type SymbolProvider,
  type TypeOracle,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import * as English from 'blockly/msg/en';

import type { DialogService as AppDialogService } from '../../app/dialogs/service';

let registered = false;

/**
 * Prepares Blockly for the editor, once: the Blocks2Cpp ID generator, the mutators, every catalog
 * block (with the expression shadows and the placeholder) and Blockly's English interface text.
 *
 * @throws IdGeneratorError when Blockly does not take the ID generator (a changed Blockly version).
 */
export function registerEditorBlocks(): void {
  if (registered) {
    return;
  }
  // Blockly's user-interface text (context menus, tooltips). The UI language becomes selectable
  // with the i18n work (docs/spec/04-user-interface.md §4.9).
  Blockly.setLocale(English as unknown as Record<string, string>);
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
  registered = true;
}

/**
 * The shadow a new value part of a variadic block shows (⊕): the input's catalog default as an
 * expression shadow marked absent, or nothing when it has no default.
 */
export const defaultInputShadow: InputShadowFactory = (_block, input) =>
  input.default.length > 0 ? exprShadowState(input.default, false, input.check, true) : null;

/** The fields' dialogs, shown by the app's accessible dialogs (never `window.prompt`). */
export function fieldDialogs(dialogs: AppDialogService): FieldDialogService {
  return {
    prompt: (message, defaultValue) => dialogs.prompt({ message, defaultValue }),
    confirm: (message) => dialogs.confirm({ message }),
    alert: (message) => dialogs.alert({ message }),
  };
}

/** What {@link installEditorServices} installs. */
export interface EditorServiceSet {
  readonly symbols: SymbolProvider;
  readonly types: TypeOracle;
  readonly dialogs: FieldDialogService;
}

/**
 * Installs the services everywhere blockly-ext reads them: the fields (`setEditorServices`), the
 * mutators (argument labels, default shadows of new parts, fresh parameter symbol IDs) and the
 * connection checker's type oracle. Returns the function that goes back to the defaults.
 */
export function installEditorServices(services: EditorServiceSet): () => void {
  const installed: EditorServices = {
    symbols: services.symbols,
    types: services.types,
    dialogs: services.dialogs,
  };
  setEditorServices(installed);
  configureMutators({
    symbols: services.symbols,
    inputShadow: defaultInputShadow,
    newSymbolId: () => newId('sym'),
  });
  setCheckerTypeOracle(services.types);
  return () => {
    resetEditorServices();
    configureMutators({ symbols: null, inputShadow: null, newSymbolId: null });
    setCheckerTypeOracle(null);
  };
}
