/**
 * The type-aware connection checker (03 §3.3, 03 §3.5.3): `B2cConnectionChecker`, registered as
 * `b2c_checker`, and the type rules it applies; the wrapping of statements in a C-block (03 §3.3)
 * with its drag preview, `B2cConnectionPreviewer`, registered as `b2c_previewer`.
 */
export { B2C_CHECKER_NAME, B2cConnectionChecker, registerB2cConnectionChecker } from './checker';
export { conversionAllowed, staticConversion } from './conversion';
export { setCheckerTypeOracle } from './oracle';
export {
  beginSlotGesture,
  blockTyping,
  candidateFit,
  currentSlotCache,
  endSlotGesture,
  setCurrentSlotCache,
  SlotCache,
  startValueExpectations,
  valueType,
} from './slots';
export { staticOutputType } from './output-type';
export {
  B2C_PREVIEWER_NAME,
  B2cConnectionPreviewer,
  previewedWrap,
  registerB2cConnectionPreviewer,
} from './previewer';
export type { Compatibility, OutputTypeOracle, StaticConversion } from './types';
export {
  completeWrap,
  firstMouth,
  isEmptyStatementCBlock,
  wrapPlace,
  type WrapPlace,
} from './wrap';
