import * as vscode from "vscode";
import { byteOffsetToJsIndex } from "./utf8";

export { byteOffsetToJsIndex, utf8ByteLength, sliceByByteOffsets } from "./utf8";

/**
 * Convert a UTF-8 byte offset (from the Rust WASM parser) into a VS Code Position.
 */
export function byteOffsetToPosition(
  document: vscode.TextDocument,
  byteOffset: number
): vscode.Position {
  if (byteOffset <= 0) return new vscode.Position(0, 0);
  const utf16Offset = byteOffsetToJsIndex(document.getText(), byteOffset);
  return document.positionAt(utf16Offset);
}

/** Convert a VS Code Position to a UTF-8 byte offset. */
export function positionToByteOffset(
  document: vscode.TextDocument,
  position: vscode.Position
): number {
  const text = document.getText();
  const utf16Target = document.offsetAt(position);
  let utf16Pos = 0;
  let bytePos = 0;

  for (let i = 0; i < text.length && utf16Pos < utf16Target; i++) {
    const code = text.charCodeAt(i);

    if (code >= 0xd800 && code <= 0xdbff) {
      utf16Pos += 2;
      bytePos += 4;
      i++;
    } else if (code <= 0x7f) {
      utf16Pos += 1;
      bytePos += 1;
    } else if (code <= 0x7ff) {
      utf16Pos += 1;
      bytePos += 2;
    } else {
      utf16Pos += 1;
      bytePos += 3;
    }
  }

  return bytePos;
}

export function byteRangeToVsRange(
  document: vscode.TextDocument,
  start: number,
  end: number
): vscode.Range {
  return new vscode.Range(
    byteOffsetToPosition(document, start),
    byteOffsetToPosition(document, end)
  );
}
