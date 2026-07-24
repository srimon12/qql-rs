const nativeBinding = require('./index.linux-x64-gnu.node');

/**
 * Parses a QQL statement into a JavaScript AST object using fast Rust-JSON serialization.
 */
function parse(query) {
    return JSON.parse(nativeBinding.parseJson(query));
}

/**
 * Parses a QQL statement and returns the AST as a raw JSON string directly from Rust.
 * Bypasses V8 object creation entirely (~1.15M ops/s). Ideal for HTTP/IPC forwarding.
 */
function parseJson(query) {
    return nativeBinding.parseJson(query);
}

/**
 * Parses multiple QQL statements into an array of JavaScript AST objects.
 */
function parseAll(queries) {
    return JSON.parse(nativeBinding.parseBatchJson(queries));
}

/**
 * Parses an array of QQL queries into an array of JavaScript AST objects.
 */
function parseBatch(queries) {
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
    parseJson,
    parseAll,
    parseBatch,
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
