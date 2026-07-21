# Node.js SDK (`nqql`) Reference & Examples

Native Node.js bindings for QQL compiled with N-API (`napi-rs`).

## Installation

```bash
npm install nqql
```

## Quick Start & Client Configuration

```javascript
const { Client, HttpEmbedder, parse, isValid, injectFilter, compileQuery, explain } = require('nqql');

// 1. Initialize HTTP Embedder
const embedder = new HttpEmbedder({
    endpoint: "http://localhost:11434/v1/embeddings",
    model: "all-minilm:l6-v2",
    dimension: 384,
    apiKey: ""
});

// 2. Initialize QQL Client
const client = new Client({
    url: "http://localhost:6333",
    apiKey: null,
    useGrpc: false,
    embedder: embedder
});

async function main() {
    // 3. Execute QQL Query
    const response = await client.execute("QUERY 'cardiology' FROM medical_records USING dense LIMIT 5");
    console.log("Response:", response);

    // 4. Lower statement to Qdrant REST route object
    const route = compileQuery("QUERY 'search' FROM docs USING dense LIMIT 10");
    console.log("Method:", route.method, "Path:", route.path, "Payload:", route.payload);
}

main();
```

## AST Parsing, Validation & Filter Injection

```javascript
const { parse, parseAll, isValid, injectFilter, tokenize } = require('nqql');

// Parse statement to Stmt object
const stmt = parse("QUERY 'search' FROM docs USING dense LIMIT 10");

// Parse semicolon-separated script
const scriptStmts = parseAll("CREATE COLLECTION docs (dense VECTOR(384, COSINE)); QUERY 'x' FROM docs;");

// Inject filter programmatically
const secured = injectFilter("QUERY 'search' FROM docs LIMIT 10", "tenant_id", "=", "acme");

// Validate syntax
if (isValid("QUERY 'search' FROM docs")) {
    console.log("Valid QQL statement");
}
```
