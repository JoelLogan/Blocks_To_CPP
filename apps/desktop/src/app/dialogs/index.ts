/**
 * The app's in-window dialogs: the {@link DialogService} every feature uses, the host component
 * that shows them, and the override of Blockly's own dialogs.
 */
export { installBlocklyDialogs } from './blockly';
export { DialogHost } from './DialogHost';
export { dialogs } from './instance';
export * from './service';
