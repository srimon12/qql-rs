const nativeBinding = require('./index.linux-x64-gnu.node');

/**
 * Parses a QQL statement and returns the AST as a Stmt class instance.
 */
function parse(query) {
    return nativeBinding.parse(query);
}

/**
 * Parses a QQL statement by serializing to JSON in Rust, then using `JSON.parse` in V8.
 * This skips `napi-rs` memory allocations and provides a significant performance 
 * boost (~236k ops/s).
 */
function parseFastJson(query) {
    return JSON.parse(nativeBinding.parseJson(query));
}

function parseAll(queries) {
    return nativeBinding.parseAll(queries);
}

function parseBatch(queries) {
    return nativeBinding.parseBatch(queries);
}

function parseBatchFastJson(queries) {
    return JSON.parse(nativeBinding.parseBatchJson(queries));
}

function isValid(query) {
    return nativeBinding.isValid(query);
}

function injectFilter(query, field, op, value) {
    return nativeBinding.injectFilter(query, field, op, value);
}

function tokenize(query) {
    return nativeBinding.tokenize(query);
}

function explain(query) {
    return nativeBinding.explain(query);
}

function explainStmt(stmt) {
    return nativeBinding.explainStmt(stmt);
}

function execute(query, options) {
    return nativeBinding.execute(query, options);
}

function executeStmt(stmt, options) {
    return nativeBinding.executeStmt(stmt, options);
}

const Client = nativeBinding.Client;
const Stmt = nativeBinding.Stmt;
const HttpEmbedder = nativeBinding.HttpEmbedder;

module.exports = {
    parse,
    parseFastJson,
    parseAll,
    parseBatch,
    parseBatchFastJson,
    isValid,
    injectFilter,
    tokenize,
    explain,
    explainStmt,
    execute,
    executeStmt,
    Client,
    Stmt,
    HttpEmbedder,
    nativeBinding
};
