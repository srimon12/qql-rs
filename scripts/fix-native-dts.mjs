#!/usr/bin/env node
// @generated-companion: post-processes napi's `native.d.ts` output.
//
// `napi build --dts native.d.ts --pipe ...` calls this script once per build
// output file. napi emits opaque `ToNapiValue` wrapper types by their Rust
// type name but cannot declare them, so `BigIntSafeJson` would reference an
// undeclared type. Append the recursive alias after generation, and otherwise
// leave non-`.d.ts` outputs alone.

import { readFileSync, writeFileSync } from "node:fs";

const DECLARATION = `
/**
 * BigInt-safe JSON value crossing the native boundary: integers a JS
 * 'number' holds exactly stay numbers, larger u64/i64 values cross as
 * 'bigint'. Appended by scripts/fix-native-dts.mjs because napi cannot
 * declare opaque ToNapiValue wrapper types.
 */
export type BigIntSafeJson =
  | null
  | boolean
  | number
  | bigint
  | string
  | BigIntSafeJson[]
  | { [key: string]: BigIntSafeJson };
`;

// Any declaration form (type alias, interface, class), exported or not, means
// the generator already owns the name — never append a duplicate.
const DECLARED = /(?:^|\n)\s*(?:export\s+)?(?:type|interface|class)\s+BigIntSafeJson\b/;

let touched = 0;
for (const path of process.argv.slice(2)) {
  if (!path.endsWith(".d.ts")) {
    continue;
  }
  const source = readFileSync(path, "utf8");
  if (!source.includes("BigIntSafeJson") || DECLARED.test(source)) {
    continue;
  }
  writeFileSync(path, `${source.trimEnd()}\n${DECLARATION}`);
  touched += 1;
}
if (touched === 0) {
  console.log("fix-native-dts: nothing to do");
}
