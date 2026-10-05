/**
 * What the trust feature says (docs/spec/08-security.md §8.3, 04 §4.10): why a project is in
 * Restricted Mode, what that means, why a trusted project is trusted, and the result of *Trust…*
 * and *Revoke trust*. Fixed texts only: nothing from the backend or the project is shown here.
 */
import type { RestrictedReason, Trust } from '@blocks2cpp/ipc-types';

import type { TrustMessage } from './controller';

/** Why a project is restricted. */
export function restrictedReasonText(reason: RestrictedReason | null): string {
  switch (reason) {
    case 'changedOutside':
      return 'Its Raw C++ blocks, libraries, packs or defines were changed outside Blocks2Cpp since you trusted it.';
    case 'noRecord':
      return 'You have not trusted this project on this computer yet. It was made somewhere else, or copied or moved here.';
    case null:
      return 'This project is not trusted on this computer.';
  }
}

/** What Restricted Mode allows and what it holds back. */
export const RESTRICTED_MODE_TEXT =
  'You can view and edit its blocks and read the C++ code, but Build and Run stay off until you trust it.';

/** The stronger warning for a file with the Windows Mark of the Web (§8.3). */
export const MARK_OF_THE_WEB_TEXT =
  'This file was downloaded from the Internet. Only trust it if you know who made it and what it does.';

/** Why a trusted project is trusted, as shown in the Settings page's *This project* section. */
export function trustedText(trust: Trust): string {
  switch (trust.source) {
    case 'project':
      return 'You trusted this project on this computer, so it can be built and run.';
    case 'folder':
      return 'This project is trusted because you trusted everything in its folder.';
    case 'createdHere':
      return 'This project was created on this computer, so it is trusted. Its trust is recorded when you save it.';
    case null:
      return 'This project is trusted.';
  }
}

/** What happened, for the message under the trust actions. */
export function trustMessageText(message: TrustMessage): string {
  switch (message.kind) {
    case 'stayedRestricted':
      return 'The project stays in Restricted Mode.';
    case 'granted':
      return 'The project is trusted now: you can build and run it.';
    case 'revoked':
      return 'Trust was revoked: the project is in Restricted Mode now.';
    case 'stillTrustedByFolder':
      return 'This project’s own trust was removed, but it is still trusted because you trusted everything in its folder.';
    case 'failed':
      return trustFailureText(message);
  }
}

function trustFailureText(message: Extract<TrustMessage, { kind: 'failed' }>): string {
  switch (message.code) {
    case 'rateLimited':
      return 'Please wait a moment, then try again.';
    case 'busy':
      return 'Another dialog is already open. Close it, then try again.';
    case 'unknownHandle':
      return 'This project is no longer open.';
    default:
      return message.action === 'grant'
        ? `The trust choice could not be recorded (${message.code}), so the project stays in Restricted Mode.`
        : `Trust could not be revoked (${message.code}).`;
  }
}

/** Whether a message reports a failure (shown as an error) or a result. */
export function isFailure(message: TrustMessage): boolean {
  return message.kind === 'failed';
}
