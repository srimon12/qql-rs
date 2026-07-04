# nqql

Node.js native bindings for the QQL parser, compiled using Neon.

---

## Features
* **High Performance**: Native Rust parsing speed inside Node.js applications.
* **Typing Support**: Exposes Javascript native objects representing the parsed AST.

---

## Installation

```bash
npm install nqql
```

---

## Usage

```javascript
const { parse } = require('nqql');

// Parse QQL query
const ast = parse("QUERY 'full text match' FROM articles LIMIT 10");
console.log(JSON.stringify(ast, null, 2));
```
