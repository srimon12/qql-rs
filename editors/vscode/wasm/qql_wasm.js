/* @ts-self-types="./qql_wasm.d.ts" */
import * as wasm from "./qql_wasm_bg.wasm";
import { __wbg_set_wasm } from "./qql_wasm_bg.js";

__wbg_set_wasm(wasm);

export {
    Client, Stmt, analyze, compile, compileBytes, explain, explainBytes, formatQuery, inject_filter, isValid, parse, tokenize
} from "./qql_wasm_bg.js";
