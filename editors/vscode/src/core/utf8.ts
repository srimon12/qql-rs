/**
 * UTF-8 byte offset helpers.
 *
 * The Rust WASM parser reports spans as UTF-8 byte offsets.
 * JavaScript strings are UTF-16 — never use byte offsets as slice indices.
 */

/** Convert a UTF-8 byte offset into a JS string index (UTF-16 code units). */
export function byteOffsetToJsIndex(text: string, byteOffset: number): number {
  if (byteOffset <= 0) return 0;

  let jsIndex = 0;
  let bytePos = 0;

  for (let i = 0; i < text.length && bytePos < byteOffset; i++) {
    const code = text.charCodeAt(i);

    if (code >= 0xd800 && code <= 0xdbff) {
      // High surrogate of a pair → 4 UTF-8 bytes, 2 UTF-16 units
      jsIndex += 2;
      bytePos += 4;
      i++; // skip low surrogate
    } else if (code <= 0x7f) {
      jsIndex += 1;
      bytePos += 1;
    } else if (code <= 0x7ff) {
      jsIndex += 1;
      bytePos += 2;
    } else {
      jsIndex += 1;
      bytePos += 3;
    }
  }

  return jsIndex;
}

/** Total UTF-8 byte length of a JS string. */
export function utf8ByteLength(text: string): number {
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      bytes += 4;
      i++;
    } else if (code <= 0x7f) {
      bytes += 1;
    } else if (code <= 0x7ff) {
      bytes += 2;
    } else {
      bytes += 3;
    }
  }
  return bytes;
}

/** Slice `text` using UTF-8 byte offsets (inclusive start, exclusive end). */
export function sliceByByteOffsets(
  text: string,
  startByte: number,
  endByte: number
): string {
  const start = byteOffsetToJsIndex(text, startByte);
  const end = byteOffsetToJsIndex(text, endByte);
  return text.slice(start, end);
}
