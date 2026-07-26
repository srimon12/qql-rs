import init, { Client, parse, compile, isValid } from 'qql-wasm';

await init();

// 1. Parse & validate (browser-side, no server)
const stmt = parse("QUERY 'wasm performance' FROM edge_docs USING dense LIMIT 10");
isValid("QUERY 'v8 engine' FROM edge_docs");

// 2. Compile to REST route (offline)
const route = compile("QUERY 'search' FROM docs USING dense LIMIT 10");
// route = "{ method: \"POST\", path: \"/collections/docs/points/query\", ... }"

// 3. Execute via browser fetch
const client = new Client("http://localhost:6333", null);
// endpoint required — no default URL
client.setHttpEmbedder("https://api.openai.com/v1/embeddings", "text-embedding-3-small", 1536, "sk-...");

const result = await client.execute(
    "QUERY 'vector databases' FROM docs USING dense LIMIT 10"
);
