import init, { Client, HttpEmbedder } from 'qql-wasm';

async function run() {
    await init();

    // 1. Client-side HttpEmbedder and Client
    const embedder = new HttpEmbedder("http://localhost:11434/v1/embeddings", "nomic-embed-text", 768);
    const client = new Client("http://localhost:6333", null, embedder);

    // 2. Complex CTE prefetch DAG
    const query = `
    WITH dense_pass AS (
        QUERY 'wasm performance' FROM edge_docs USING DENSE LIMIT 10
    ),
    sparse_pass AS (
        QUERY 'v8 engine' FROM edge_docs USING SPARSE LIMIT 10
    )
    QUERY FUSION rrf LIMIT 5
    `;

    // 3. Compile REST payload & inspect plan in browser
    const payload = client.compile(query);
    console.log("Compiled REST Payload:", payload);

    const plan = client.explain(query);
    console.log("Execution Plan:\n", plan);
}

run();
