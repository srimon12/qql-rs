# nqql-edge

Native Node.js bindings for local QQL execution. `nqql-edge` combines the QQL
runtime with `qdrant-edge` for in-process vector storage and FastEmbed for
optional local ONNX inference.

## Installation

```bash
npm install nqql-edge
```

Embedding models are downloaded on first use and cached locally. Model weights
are not included in the npm package.

Prebuilt native packages are provided for Linux x64 (glibc), Windows x64, and
Apple Silicon macOS. ONNX Runtime does not provide the required macOS Intel
artifact, so `nqql-edge` does not publish a Darwin x64 package.

## Quick start

```javascript
const { localExecutor } = require('nqql-edge');

const client = localExecutor('./qql-data', {
  model: 'bge-small-en-v1.5',
  onDiskPayload: true,
});

await client.execute('CREATE COLLECTION docs HYBRID');
await client.execute(
  'UPSERT INTO docs VALUES {id: 1, text: "hello from edge"}',
);
const report = await client.execute(
  "QUERY 'hello' FROM docs USING dense LIMIT 5",
);

console.log(report);
await client.close();
```

The package also exposes QQL parsing, validation, tokenization, filter
injection, planning, and model discovery. See the
[repository documentation](https://github.com/srimon12/qql-rs/tree/main/crates/nqql-edge)
for the complete API.

## License

MIT
