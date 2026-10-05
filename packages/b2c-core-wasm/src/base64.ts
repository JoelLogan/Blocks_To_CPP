/**
 * A strict base64 decoder (RFC 4648 §4, standard alphabet, `=` padding required) for the embedded
 * WebAssembly bytes. It decodes straight into a byte array, without the intermediate binary
 * string of `atob`, and refuses anything that is not canonical base64.
 */

const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
const PAD = 0x3d; // '='

/** The 6-bit value of each ASCII character, or -1. */
const SEXTETS: readonly number[] = Array.from({ length: 128 }, (_, code) =>
  ALPHABET.indexOf(String.fromCharCode(code)),
);

function sextet(text: string, index: number): number {
  const value = SEXTETS[text.charCodeAt(index)] ?? -1;
  if (value < 0) {
    throw new TypeError(`not base64: unexpected character at offset ${String(index)}`);
  }
  return value;
}

/**
 * Decodes base64 text into bytes.
 *
 * @throws TypeError when the length is not a multiple of 4, a character is outside the alphabet,
 *   padding appears anywhere but at the end, or the unused bits before the padding are not zero.
 */
export function decodeBase64(text: string): Uint8Array<ArrayBuffer> {
  if (text.length % 4 !== 0) {
    throw new TypeError('not base64: the length is not a multiple of 4');
  }
  let padding = 0;
  if (text.length > 0 && text.charCodeAt(text.length - 1) === PAD) {
    padding = text.charCodeAt(text.length - 2) === PAD ? 2 : 1;
  }
  const bytes = new Uint8Array((text.length / 4) * 3 - padding);
  const fullGroups = padding === 0 ? text.length / 4 : text.length / 4 - 1;
  let out = 0;
  for (let group = 0; group < fullGroups; group++) {
    const at = group * 4;
    const bits =
      (sextet(text, at) << 18) |
      (sextet(text, at + 1) << 12) |
      (sextet(text, at + 2) << 6) |
      sextet(text, at + 3);
    bytes[out++] = bits >>> 16;
    bytes[out++] = (bits >>> 8) & 0xff;
    bytes[out++] = bits & 0xff;
  }
  if (padding > 0) {
    const at = text.length - 4;
    const first = sextet(text, at);
    const second = sextet(text, at + 1);
    bytes[out] = (first << 2) | (second >>> 4);
    if (padding === 1) {
      const third = sextet(text, at + 2);
      bytes[out + 1] = ((second & 0x0f) << 4) | (third >>> 2);
      if ((third & 0x03) !== 0) {
        throw new TypeError('not base64: non-zero bits before the padding');
      }
    } else if ((second & 0x0f) !== 0) {
      throw new TypeError('not base64: non-zero bits before the padding');
    }
  }
  return bytes;
}
