const nativeBinding = require('./index.linux-x64-gnu.node');

/**
 * Parses a QQL statement and returns the AST as native Javascript Objects.
 * This is the standard binding using `napi-rs` serialization (~60k ops/s).
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

function execute(query, options) {
    return nativeBinding.execute(query, options);
}

const Client = nativeBinding.Client;
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
    execute,
    Client,
    HttpEmbedder,
    // raw export for native bindings if required
    nativeBinding
};
