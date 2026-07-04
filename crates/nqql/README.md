# nqql

Node.js native bindings for the QQL parser, compiled using N-API (`napi-rs`).

## Features

- **Native parsing**: Rust-speed QQL parsing in Node.js
- **Tokenization**: Access raw lexer tokens
- **Filter injection**: Add tenant isolation filters to parsed queries
- **Validation**: Check if a query string is valid QQL

## Installation

```bash
npm install nqql
```

## Usage

```javascript
const nqql = require('nqql');

// Parse to debug-formatted AST
const ast = nqql.parse("QUERY 'full text match' FROM articles LIMIT 10");
console.log(ast);

// Parse multiple statements
const stmts = nqql.parseAll("INSERT INTO docs ...; QUERY 'text' FROM docs ...");
console.log(stmts);

// Validate without parsing
const valid = nqql.isValid("SELECT * FROM docs WHERE id = 1");

// Inject filter (tenant isolation)
const secured = nqql.injectFilter(
    "QUERY 'search' FROM docs LIMIT 10",
    "org_id",
    "=",
    '"acme-corp"'
);

// Tokenize
const tokens = nqql.tokenize("QUERY 'hello' FROM docs LIMIT 5");
```

## API

| Function | Returns | Description |
|---|---|---|
| `parse(input)` | `string` | Parse single statement → debug AST |
| `parseAll(input)` | `string[]` | Parse multiple semicolon-separated statements |
| `parseBatch(queries)` | `string[]` | Parse an array of query strings |
| `isValid(input)` | `boolean` | Check if query string is valid QQL |
| `injectFilter(query, field, op, value)` | `string` | Inject filter into query AST |
| `tokenize(input)` | `string` | Tokenize query string (JSON) |
