/**
 * The app's in-window dialogs: the {@link DialogService} every feature uses, the host component
 * that shows them, the override of Blockly's own dialogs, and the editor plugin that closes
 * Blockly's pop-ups before a dialog opens.
 */
export { blocklyPopupsPlugin, installBlocklyDialogs } from './blockly';
export { DialogHost } from './DialogHost';
export { dialogs } from './instance';
export * from './service';
