/**
 * Two-way highlighting between the blocks and the C++ (docs/spec/04-user-interface.md §4.3): the
 * hovered and selected block go to the store for the code panel, and clicks in the code or in
 * Problems select their block.
 */
export { attachHighlightTracking, blockIdAt, documentBlockId } from './tracking';
export { revealTarget, selectBlockFromCode, selectBlockFromProblem, visibleHolder } from './reveal';
