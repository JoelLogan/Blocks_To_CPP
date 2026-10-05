/**
 * The type-aware connection checker (03 §3.3, 03 §3.5.3): `B2cConnectionChecker`, registered as
 * `b2c_checker`, and the type rules it applies.
 */
export { B2C_CHECKER_NAME, B2cConnectionChecker, registerB2cConnectionChecker } from './checker';
export { conversionAllowed, staticConversion } from './conversion';
export { setCheckerTypeOracle } from './oracle';
export { staticOutputType } from './output-type';
export type { Compatibility, OutputTypeOracle, StaticConversion } from './types';
