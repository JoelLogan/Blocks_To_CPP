/** The app's dialog queue, shown by the `DialogHost` at the root of the window. */
import { createDialogQueue, type DialogQueue } from './service';

export const dialogs: DialogQueue = createDialogQueue();
