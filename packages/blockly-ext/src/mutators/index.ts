/**
 * The variadic ⊕/⊖ mutators (03 §3.2, 05 §5.4 `extra`): `registerB2cMutators()` and what the
 * editor needs to work with mutated blocks.
 */
export { MutatorButton } from './buttons';
export { MutatorConfigError, MutatorStateError, type MutatorStateProblem } from './errors';
export {
  configureMutators,
  type InputShadowFactory,
  type MutatorHooks,
  type MutatorSymbolInfo,
  type MutatorSymbols,
} from './hooks';
export { isB2cMutatorBlock, refreshMutatorLabels, registerB2cMutators } from './register';
export { type LabelRegion, mutatorLabelRegion } from './spec';
export { MAX_VARIADIC_PARTS } from './state';
export {
  B2C_MUTATOR_CALL_ARGS,
  B2C_MUTATOR_IF,
  B2C_MUTATOR_ITEMS,
  B2C_MUTATOR_NAMES,
  B2C_MUTATOR_PARAMS,
  type B2cMutatorBlock,
  type B2cMutatorName,
  PARAM_MODES,
  type ParamMode,
  type ParamRow,
} from './types';
