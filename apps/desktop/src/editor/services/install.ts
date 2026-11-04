/**
 * Registering the Blocks2Cpp blocks with Blockly, and installing the editor services into
 * blockly-ext: the fields' symbol and type services and dialogs, the mutators' hooks and the
 * connection checker's type oracle (wave-2 seams of blockly-ext).
 */
import {
  type DeclNameFactory,
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
import { candidateName, defaultNameKind } from '../toolbox/names';

let registered = false;

/**
 * Prepares Blockly for the editor, once: the Blocks2Cpp ID generator, the mutators, every catalog
 * block (with the expression shadows and the placeholder) and Blockly's English interface text.
 *
 * On every call it also gives the mutators the editor's default names of new declaration parts
 * ({@link defaultDeclName}), which a closed editor withdrew with its services: the toolbox's flyout
 * builds its blocks while `Blockly.inject` runs, before {@link installEditorServices}, and keeps
 * them from one showing to the next (../toolbox/inflater.ts). Built without the hook, *try*'s
 * catch part kept the mutators' own name, `name`, instead of `e`. The other hooks need not come
 * first: a toolbox item gives every value part its shadow, and each showing renews the symbol IDs.
 *
 * @throws IdGeneratorError when Blockly does not take the ID generator (a changed Blockly version).
 */
export function registerEditorBlocks(): void {
  configureMutators({ declName: defaultDeclName });
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

/**
 * The name a new part of a repeated declaration field starts with (a second catch part's `e`,
 * docs/plans/m3-contracts/w2.md §2.1): the first default name of its kind (../toolbox/names.ts),
 * or nothing for a field without one.
 */
export const defaultDeclName: DeclNameFactory = (block, field) => {
  const kind = defaultNameKind(block.type, field);
  return kind === undefined ? null : candidateName(kind, 0);
};

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
    declName: defaultDeclName,
  });
  setCheckerTypeOracle(services.types);
  return () => {
    resetEditorServices();
    configureMutators({ symbols: null, inputShadow: null, newSymbolId: null, declName: null });
    setCheckerTypeOracle(null);
  };
}
